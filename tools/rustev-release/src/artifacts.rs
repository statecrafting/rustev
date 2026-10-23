//! Artifact comparison, npm closure audit and SBOM coverage (031 3.9.1 to 3.9.3).
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use crate::ReleaseSet;
use crate::archive::{sha256_hex, unpack_bytes};
use crate::repro::fresh_dir;

type Files = BTreeMap<String, Vec<u8>>;

/// 031 3.9.3: collect every file, refusing links and ambiguous path names.
pub fn files(dir: &Path) -> Result<Files, String> {
    fn visit(base: &Path, dir: &Path, out: &mut Files) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                visit(base, &path, out)?;
            } else if kind.is_file() {
                let rel = path.strip_prefix(base).map_err(|e| e.to_string())?;
                let name = rel
                    .components()
                    .map(|part| part.as_os_str().to_str().ok_or("non-UTF8 artifact path"))
                    .collect::<Result<Vec<_>, _>>()?
                    .join("/");
                if name.chars().any(char::is_control) {
                    return Err(format!("invalid artifact path {name:?}"));
                }
                out.insert(name, std::fs::read(&path).map_err(|e| e.to_string())?);
            } else {
                return Err(format!(
                    "{} is not a regular file or directory",
                    path.display()
                ));
            }
        }
        Ok(())
    }
    let mut out = Files::new();
    visit(dir, dir, &mut out)?;
    Ok(out)
}

/// 031 3.9.3: both original digests are evidence, even when entries match.
#[derive(Debug, Serialize)]
pub struct Artifact {
    pub name: String,
    pub sha256_a: Option<String>,
    pub sha256_b: Option<String>,
    pub differences: Vec<String>,
}

/// 031 3.9.3: stable JSON report, including identical artifacts.
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub artifacts: Vec<Artifact>,
    pub problems: Vec<String>,
}

fn unpack(bytes: &[u8], name: &str) -> Result<Files, String> {
    let scratch = fresh_dir("rustev-artifact")?;
    let mut entries: Files = unpack_bytes(bytes, Path::new(name), &scratch, ".")?
        .into_iter()
        .map(|f| (f.path, f.bytes))
        .collect();
    // 031 3.9.3: empty directories are entries too, not container metadata.
    fn directories(base: &Path, dir: &Path, entries: &mut Files) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                let path = entry.path();
                let relative = path.strip_prefix(base).map_err(|e| e.to_string())?;
                entries.insert(
                    format!("{}/", relative.to_string_lossy().replace('\\', "/")),
                    vec![],
                );
                directories(base, &path, entries)?;
            }
        }
        Ok(())
    }
    directories(&scratch, &scratch, &mut entries)?;
    Ok(entries)
}

fn differences(a: &Files, b: &Files) -> Vec<String> {
    a.keys()
        .chain(b.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|name| match (a.get(name), b.get(name)) {
            (Some(x), Some(y)) if x != y => Some(format!("031 3.9.3: {name}: content differs")),
            (Some(_), None) => Some(format!("031 3.9.3: {name}: only in a")),
            (None, Some(_)) => Some(format!("031 3.9.3: {name}: only in b")),
            _ => None,
        })
        .collect()
}

/// 031 3.9.3: no filtering or normalization of either artifact set.
pub fn compare(a: &Path, b: &Path) -> Result<Report, String> {
    let (a, b) = (files(a)?, files(b)?);
    let mut report = Report {
        schema: "rustev.artifact-reproducibility/1",
        artifacts: vec![],
        problems: vec![],
    };
    if a.is_empty() || b.is_empty() {
        report
            .problems
            .push("031 3.9.3: an artifact set is empty".into());
    }
    for name in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
        let (x, y) = (a.get(name), b.get(name));
        let mut d = Vec::new();
        match (x, y) {
            (Some(x), Some(y)) if x != y => {
                if name.ends_with(".tgz") || name.ends_with(".tar.gz") {
                    match (unpack(x, name), unpack(y, name)) {
                        (Ok(x), Ok(y)) => d = differences(&x, &y),
                        (x, y) => {
                            for (run, result) in [("a", x), ("b", y)] {
                                if let Err(e) = result {
                                    d.push(format!("031 3.9.3: {name}: run {run}: {e}"));
                                }
                            }
                        }
                    }
                    if d.is_empty() {
                        d.push(
                            "031 3.9.3: container metadata differs; every file entry is identical"
                                .into(),
                        );
                    }
                } else {
                    d.push("031 3.9.3: content differs".into());
                }
            }
            (None, _) => d.push("031 3.9.3: only in b".into()),
            (_, None) => d.push("031 3.9.3: only in a".into()),
            _ => {}
        }
        report
            .problems
            .extend(d.iter().map(|d| format!("{name}: {d}")));
        report.artifacts.push(Artifact {
            name: name.clone(),
            sha256_a: x.map(|x| sha256_hex(x)),
            sha256_b: y.map(|y| sha256_hex(y)),
            differences: d,
        });
    }
    Ok(report)
}

fn platform_names(set: &ReleaseSet) -> BTreeSet<&str> {
    set.npm
        .iter()
        .filter(|p| p.directory.starts_with("integrations/rustev-node/npm/"))
        .map(|p| p.name.as_str())
        .collect()
}

fn audit_manifest(set: &ReleaseSet, p: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    if !p.is_object() {
        errors.push("package.json is not an object".into());
    }
    for key in [
        "dependencies",
        "peerDependencies",
        "bundleDependencies",
        "bundledDependencies",
    ] {
        if p.get(key).is_some() {
            errors.push(format!("forbidden {key}"));
        }
    }
    if let Some(scripts) = p.get("scripts") {
        if let Some(scripts) = scripts.as_object() {
            for key in ["preinstall", "install", "postinstall", "prepare"] {
                if scripts.contains_key(key) {
                    errors.push(format!("forbidden lifecycle script {key}"));
                }
            }
        } else {
            errors.push("scripts is not an object".into());
        }
    }
    if let Some(optional) = p.get("optionalDependencies") {
        if let Some(optional) = optional.as_object() {
            for (name, version) in optional {
                if !platform_names(set).contains(name.as_str())
                    || version.as_str() != Some(&set.version)
                {
                    errors.push(format!(
                        "optionalDependencies {name} must be a release-set platform at exactly {}",
                        set.version
                    ));
                }
            }
        } else {
            errors.push("optionalDependencies is not an object".into());
        }
    }
    errors
}

/// 031 3.9.2: inspect packed bytes, including empty node_modules directories.
pub fn npm_audit(set: &ReleaseSet, dir: &Path) -> Result<Vec<String>, String> {
    let mut errors = Vec::new();
    let files = files(dir)?;
    let mut count = 0;
    for (name, bytes) in files.iter().filter(|(n, _)| n.ends_with(".tgz")) {
        count += 1;
        let result = (|| -> Result<Vec<String>, String> {
            let scratch = fresh_dir("rustev-npm-audit")?;
            let archive = scratch.join("package.tgz");
            std::fs::write(&archive, bytes).map_err(|e| e.to_string())?;
            crate::archive::check_entries(&archive)?;
            let listing = std::process::Command::new("tar")
                .arg("-tzf")
                .arg(&archive)
                .output()
                .map_err(|e| e.to_string())?;
            if !listing.status.success() {
                return Err("tar listing failed".into());
            }
            let mut problems = Vec::new();
            for entry in String::from_utf8_lossy(&listing.stdout).lines() {
                if entry.split('/').any(|p| p == "node_modules") {
                    problems.push(format!("forbidden node_modules entry {entry}"));
                }
            }
            let entries = unpack(bytes, name)?;
            let manifest = entries
                .get("package/package.json")
                .ok_or("no package/package.json")?;
            let p: Value = serde_json::from_slice(manifest).map_err(|e| e.to_string())?;
            problems.extend(audit_manifest(set, &p));
            Ok(problems)
        })();
        let problems = result.unwrap_or_else(|e| vec![e]);
        errors.extend(
            problems
                .into_iter()
                .map(|e| format!("031 3.9.2: {name}: {e}")),
        );
    }
    if count == 0 {
        errors.push("031 3.9.2: no npm tarballs".into());
    }
    Ok(errors)
}

/// 031 3.9.1 and section 5: every supplied release archive needs a valid SBOM.
pub fn sbom_check(artifacts: &Path, sboms: &Path) -> Result<Vec<String>, String> {
    let mut errors = Vec::new();
    let mut count = 0;
    for name in files(artifacts)?
        .keys()
        .filter(|n| n.ends_with(".tgz") || n.ends_with(".tar.gz"))
    {
        count += 1;
        let path = sboms.join(format!("{name}.cdx.json"));
        let result = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string()));
        match result {
            Ok(p) if p["bomFormat"] == "CycloneDX" && p["specVersion"] == "1.5" => {}
            _ => errors.push(format!(
                "031 3.9.1: {name}: missing or invalid CycloneDX 1.5 SBOM {}",
                path.display()
            )),
        }
    }
    if count == 0 {
        errors.push("031 3.9.1: no release archives".into());
    }
    Ok(errors)
}

/// 031 3.9.1: deterministic JavaScript-only package SBOM, from its packed manifest.
pub fn main_sbom(set: &ReleaseSet, tarball: &Path, epoch: u64) -> Result<Value, String> {
    if epoch > 253_402_300_799 {
        return Err("031 3.9.1: source date exceeds RFC 3339 year range".into());
    }
    let bytes = std::fs::read(tarball).map_err(|e| e.to_string())?;
    let entries = unpack(&bytes, &tarball.display().to_string())?;
    let p: Value = serde_json::from_slice(
        entries
            .get("package/package.json")
            .ok_or("no package/package.json")?,
    )
    .map_err(|e| e.to_string())?;
    let errors = audit_manifest(set, &p);
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    if p["name"] != "@statecrafting/rustev" || p["version"] != set.version {
        return Err("main package identity differs from release set".into());
    }
    let optional = p["optionalDependencies"]
        .as_object()
        .ok_or("missing optionalDependencies")?;
    if optional.keys().map(String::as_str).collect::<BTreeSet<_>>() != platform_names(set) {
        return Err("main package must name every platform package".into());
    }
    let component = |name: &str| json!({"type":"library", "name":name, "version":set.version, "purl":format!("pkg:npm/{}@{}", name.replace('@', "%40"), set.version)});
    let timestamp = format!(
        "{}T{:02}:{:02}:{:02}Z",
        crate::audit::civil_date(epoch),
        epoch / 3600 % 24,
        epoch / 60 % 60,
        epoch % 60
    );
    Ok(
        json!({"bomFormat":"CycloneDX", "specVersion":"1.5", "version":1,
        "metadata":{"timestamp":timestamp, "component":component("@statecrafting/rustev")},
        "components":optional.keys().map(|n| component(n)).collect::<Vec<_>>() }),
    )
}
