//! The crates.io adapter of the publisher (spec 021, 3.9): visibility from
//! the public sparse index, uploads of the exact verified archive bytes.
//!
//! `cargo publish` rebuilds the archive, which 3.9 forbids, so the upload
//! speaks the registry's publish API directly: the archive's own normalized
//! manifest supplies the metadata, and the body carries the verified
//! `.crate` bytes unchanged. HTTP goes through the system `curl`; no HTTP
//! crate enters the repository's tools (spec 001, 3.4.2).
//!
//! The registry token is read from `CARGO_REGISTRY_TOKEN` only inside
//! [`CratesIo::upload`] and [`CratesIo::account`], is passed to `curl` on its
//! standard input as a header file, and never enters an argument, a log or
//! the journal (3.1, 3.9).

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::lifecycle::{Lookup, Registry, Upload};

pub const API: &str = "https://crates.io/api/v1";
pub const INDEX: &str = "https://index.crates.io";
pub const USER_AGENT: &str = "rustev-release (https://github.com/statecrafting/rustev)";

/// The sparse-index path of a crate name.
pub fn index_path(name: &str) -> String {
    let n = name.to_ascii_lowercase();
    match n.len() {
        1 => format!("1/{n}"),
        2 => format!("2/{n}"),
        3 => format!("3/{}/{n}", &n[..1]),
        _ => format!("{}/{}/{n}", &n[..2], &n[2..4]),
    }
}

#[derive(Debug, Deserialize)]
struct IndexLine {
    vers: String,
    cksum: String,
}

/// The checksum the index records for `version`, from the index file text.
pub fn index_checksum(text: &str, version: &str) -> Option<String> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<IndexLine>(l).ok())
        .find(|l| l.vers == version)
        .map(|l| l.cksum)
}

/// The publish API's metadata of one crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewCrate {
    pub name: String,
    pub vers: String,
    pub deps: Vec<NewDep>,
    pub features: BTreeMap<String, Vec<String>>,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub documentation: Option<String>,
    pub homepage: Option<String>,
    pub readme: Option<String>,
    pub readme_file: Option<String>,
    pub keywords: Vec<String>,
    pub categories: Vec<String>,
    pub license: Option<String>,
    pub license_file: Option<String>,
    pub repository: Option<String>,
    pub badges: BTreeMap<String, BTreeMap<String, String>>,
    pub links: Option<String>,
    pub rust_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewDep {
    pub name: String,
    pub version_req: String,
    pub features: Vec<String>,
    pub optional: bool,
    pub default_features: bool,
    pub target: Option<String>,
    pub kind: String,
    pub registry: Option<String>,
    pub explicit_name_in_toml: Option<String>,
}

/// `cargo metadata --no-deps` of an unpacked archive, the fields read here.
#[derive(Debug, Deserialize)]
pub struct ArchiveMetadata {
    pub packages: Vec<ArchivePackage>,
}

#[derive(Debug, Deserialize)]
pub struct ArchivePackage {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub documentation: Option<String>,
    pub homepage: Option<String>,
    pub readme: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    pub license: Option<String>,
    pub license_file: Option<String>,
    pub repository: Option<String>,
    pub links: Option<String>,
    pub rust_version: Option<String>,
    #[serde(default)]
    pub features: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub dependencies: Vec<ArchiveDep>,
}

#[derive(Debug, Deserialize)]
pub struct ArchiveDep {
    pub name: String,
    pub req: String,
    pub kind: Option<String>,
    pub rename: Option<String>,
    pub optional: bool,
    pub uses_default_features: bool,
    #[serde(default)]
    pub features: Vec<String>,
    pub target: Option<String>,
    pub registry: Option<String>,
}

/// The publish metadata of an archive from its own normalized manifest;
/// `readme` is the README text the archive carries.
pub fn new_crate(p: &ArchivePackage, readme: Option<String>) -> NewCrate {
    let mut deps: Vec<NewDep> = p
        .dependencies
        .iter()
        .map(|d| NewDep {
            name: d.name.clone(),
            version_req: d.req.clone(),
            features: d.features.clone(),
            optional: d.optional,
            default_features: d.uses_default_features,
            target: d.target.clone(),
            kind: d.kind.clone().unwrap_or_else(|| "normal".into()),
            registry: d.registry.clone(),
            explicit_name_in_toml: d.rename.clone(),
        })
        .collect();
    deps.sort_by(|a, b| (&a.name, &a.kind).cmp(&(&b.name, &b.kind)));
    NewCrate {
        name: p.name.clone(),
        vers: p.version.clone(),
        deps,
        features: p.features.clone(),
        authors: p.authors.clone(),
        description: p.description.clone(),
        documentation: p.documentation.clone(),
        homepage: p.homepage.clone(),
        readme,
        // The path as the normalized manifest names it, unchanged.
        readme_file: p.readme.clone(),
        keywords: p.keywords.clone(),
        categories: p.categories.clone(),
        license: p.license.clone(),
        license_file: p.license_file.clone(),
        repository: p.repository.clone(),
        badges: BTreeMap::new(),
        links: p.links.clone(),
        rust_version: p.rust_version.clone(),
    }
}

/// The publish request body: the metadata JSON and the archive, each
/// preceded by its length as a little-endian `u32`.
pub fn publish_body(meta: &NewCrate, archive: &[u8]) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(meta).map_err(|e| e.to_string())?;
    let len = |n: usize| {
        u32::try_from(n)
            .map(u32::to_le_bytes)
            .map_err(|_| "too large".to_string())
    };
    let mut body = Vec::with_capacity(json.len() + archive.len() + 8);
    body.extend(len(json.len())?);
    body.extend(&json);
    body.extend(len(archive.len())?);
    body.extend(archive);
    Ok(body)
}

/// How a publish response maps onto the journal's outcomes: a 2xx is
/// accepted, a 4xx with a reason is refused, and anything else (a 5xx, no
/// status, a transport failure) is ambiguous: the registry may have taken
/// it, so public state is queried before anything else happens.
pub fn classify(status: Option<u16>, body: &str) -> Upload {
    match status {
        Some(s) if (200..300).contains(&s) => Upload::Accepted,
        Some(s) if (400..500).contains(&s) => Upload::Refused {
            reason: format!("HTTP {s}: {}", body.chars().take(500).collect::<String>()),
        },
        Some(s) => Upload::Ambiguous {
            detail: format!("HTTP {s}"),
        },
        None => Upload::Ambiguous {
            detail: "no response".into(),
        },
    }
}

/// The real registry. `wait` sleeps; nothing here runs in tests.
pub struct CratesIo {
    pub wait_seconds: u64,
}

fn curl() -> Command {
    let mut c = Command::new("curl");
    c.args(["-q", "-sS", "--max-time", "120", "-A", USER_AGENT]);
    c
}

/// Run curl with the token header supplied on standard input (`-H @-`),
/// returning the HTTP status and body.
fn with_token(mut cmd: Command, body: Option<&[u8]>) -> Result<(Option<u16>, String), String> {
    let token = std::env::var("CARGO_REGISTRY_TOKEN").map_err(|e| match e {
        std::env::VarError::NotPresent => "CARGO_REGISTRY_TOKEN is not set".to_string(),
        std::env::VarError::NotUnicode(_) => {
            "CARGO_REGISTRY_TOKEN is not valid Unicode".to_string()
        }
    })?;
    // One header line: a token holding a line break or other control
    // character could add headers, so it is refused, not sent.
    // A crates.io token is ASCII; anything else is refused here, before
    // any contact, rather than by the registry as a rejection.
    if token.is_empty() || !token.is_ascii() || token.chars().any(char::is_control) {
        return Err(
            "CARGO_REGISTRY_TOKEN is empty or holds a control or non-ASCII character".into(),
        );
    }
    // A `Scratch` guard: removed when it drops at the end of this scope,
    // on every return path.
    let dir = crate::repro::fresh_dir("rustev-release-upload")?;
    let body_file = dir.join("body");
    if let Some(b) = body {
        std::fs::write(&body_file, b).map_err(|e| e.to_string())?;
        cmd.arg("--data-binary")
            .arg(format!("@{}", body_file.display()));
    }
    let mut child = cmd
        .args(["-H", "@-", "-w", "\n%{http_code}"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run curl: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("no stdin")?
        .write_all(format!("Authorization: {token}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (resp, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    Ok((
        code.trim().parse().ok().filter(|c| *c != 0),
        resp.to_string(),
    ))
}

impl CratesIo {
    /// The account the token belongs to (3.9: the operator verifies it).
    pub fn account(&self) -> Result<String, String> {
        let mut cmd = curl();
        cmd.arg(format!("{API}/me"));
        let (status, body) = with_token(cmd, None)?;
        if status != Some(200) {
            return Err(format!("account query failed: {status:?}"));
        }
        let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        v["user"]["login"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| "no login in the account response".into())
    }

    /// The metadata of the archive at `crate_path`, from its normalized
    /// manifest, read outside any workspace.
    pub fn metadata_of(crate_path: &std::path::Path, root: &str) -> Result<NewCrate, String> {
        // A `Scratch` guard: removed when it drops at the end of this scope,
        // on every return path.
        let dir = crate::repro::fresh_dir("rustev-release-meta")?;
        crate::archive::unpack(crate_path, &dir, root)?;
        let out = Command::new("cargo")
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--offline",
                "--manifest-path",
            ])
            .arg(dir.join(root).join("Cargo.toml"))
            .output()
            .map_err(|e| format!("could not run cargo metadata: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned());
        }
        let m: ArchiveMetadata = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
        let p = m
            .packages
            .into_iter()
            .next()
            .ok_or("no package in the archive")?;
        // The README is read from inside the unpacked archive only: a path
        // that is absolute or climbs out of it is refused, never read.
        let readme = match p.readme.as_deref() {
            None => None,
            Some(r) => {
                let inside = !r.is_empty()
                    && std::path::Path::new(r)
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_)));
                if !inside {
                    return Err(format!("readme {r:?} is not a path inside the archive"));
                }
                Some(
                    std::fs::read_to_string(dir.join(root).join(r))
                        .map_err(|e| format!("readme {r}: {e}"))?,
                )
            }
        };
        Ok(new_crate(&p, readme))
    }
}

impl Registry for CratesIo {
    fn lookup(&mut self, name: &str, version: &str) -> Lookup {
        // A 404 is the only absence; any other failure is unknown state.
        let out = curl()
            .args(["-w", "\n%{http_code}"])
            .arg(format!("{INDEX}/{}", index_path(name)))
            .output();
        let o = match out {
            Ok(o) => o,
            Err(e) => {
                return Lookup::Unknown {
                    detail: format!("curl: {e}"),
                };
            }
        };
        if !o.status.success() {
            return Lookup::Unknown {
                detail: "index transport failed".into(),
            };
        }
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
        match code.trim() {
            "200" => match index_checksum(body, version) {
                Some(checksum) => Lookup::Visible { checksum },
                None => Lookup::Absent,
            },
            "404" => Lookup::Absent,
            other => Lookup::Unknown {
                detail: format!("index HTTP {other}"),
            },
        }
    }

    fn upload(&mut self, name: &str, version: &str, archive: &[u8]) -> Upload {
        let root = format!("{name}-{version}");
        let meta = (|| -> Result<Vec<u8>, String> {
            // A `Scratch` guard: removed when it drops at the end of this scope,
            // on every return path.
            let dir = crate::repro::fresh_dir("rustev-release-crate")?;
            let path = dir.join(format!("{root}.crate"));
            std::fs::write(&path, archive).map_err(|e| e.to_string())?;
            publish_body(&Self::metadata_of(&path, &root)?, archive)
        })();
        // A failure before any contact with the registry is a refusal whose
        // reason begins `not sent:`, which the journal records verbatim, so
        // a reader tells it from a registry's rejection.
        let body = match meta {
            Ok(b) => b,
            Err(e) => {
                return Upload::Refused {
                    reason: format!("not sent: {e}"),
                };
            }
        };
        let mut cmd = curl();
        // The body is binary (two length-prefixed parts), never a form;
        // `--data-binary` would otherwise label it urlencoded.
        cmd.args(["-X", "PUT", "-H", "Content-Type: application/octet-stream"])
            .arg(format!("{API}/crates/new"));
        match with_token(cmd, Some(&body)) {
            Ok((status, resp)) => classify(status, &resp),
            Err(e) => Upload::Refused {
                reason: format!("not sent: {e}"),
            },
        }
    }

    fn wait(&mut self) {
        std::thread::sleep(std::time::Duration::from_secs(self.wait_seconds));
    }
}
