//! Spec 031 3.1 and 3.9, 058 3.2: the run's artifacts, bound into its manifest.
use crate::{
    ReleaseSet,
    archive::sha256_hex,
    manifest::{ReleaseManifest, WorkflowArtifact},
};
use sha2::{Digest, Sha512};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const PLATFORMS: &[&str] = &[
    "darwin-arm64",
    "darwin-x64",
    "linux-arm64-gnu",
    "linux-arm64-musl",
    "linux-x64-gnu",
    "linux-x64-musl",
    "win32-x64-msvc",
];
pub const MISSING: &str =
    "Missing rehearsal artifacts: npm, binary (unless absent), workflow SBOM; not publishable.";

/// Spec 031 3.1: npm's SHA-512 SRI, using the existing SHA dependency.
pub fn integrity(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digest = Sha512::digest(bytes);
    let mut out = String::from("sha512-");
    for c in digest.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((c.get(1).copied().unwrap_or(0) as u32) << 8)
            | c.get(2).copied().unwrap_or(0) as u32;
        for shift in [18, 12, 6, 0] {
            out.push(
                if (shift == 6 && c.len() < 2) || (shift == 0 && c.len() < 3) {
                    '='
                } else {
                    ALPHABET[((n >> shift) & 63) as usize] as char
                },
            );
        }
    }
    out
}

pub fn plain(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['.', '-'])
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// Spec 031 3.9.5: level 3 needs an isolated workflow that does not exist.
pub fn validate(m: &ReleaseManifest) -> Result<(), String> {
    let candidate = crate::candidate::is_candidate_version(&m.version);
    if candidate {
        let set = ReleaseSet::parse(include_bytes!("../release-set.json"))?;
        let c = crate::candidate::parse(&m.tag, &set.version)?;
        if c.version != m.version || !m.packages.is_empty() {
            return Err(
                "053 3.2.3: candidate packages must be empty and version must match tag".into(),
            );
        }
    } else if m.packages.is_empty() {
        return Err("031 3.11.2: release packages must not be empty".into());
    }
    if m.registries
        != [
            m.registry.clone(),
            "https://registry.npmjs.org".into(),
            crate::REPOSITORY.to_string() + "/releases",
            "ghcr.io/statecrafting".into(),
        ]
    {
        return Err("unexpected registries".into());
    }
    for p in &m.packages {
        if !plain(&p.name) || p.crate_file != format!("{}-{}.crate", p.name, m.version) {
            return Err(format!(
                "{} is not <member>-<version>.crate or has an invalid member",
                p.crate_file
            ));
        }
    }
    if !plain(&m.version) || m.evidence.keys().any(|n| !plain(n)) {
        return Err("invalid version or evidence path".into());
    }
    let mut names = BTreeSet::new();
    for a in &m.artifacts {
        if !plain(&a.name) || !names.insert(&a.name) {
            return Err(format!("invalid or duplicate artifact {}", a.name));
        }
        let layer = match a.kind.as_str() {
            "npm" => 6,
            "binary" => 7,
            "sbom" | "changelog" => 8,
            "image" => return Err(
                "031 3.9.5; 053 3.4.2: no container image is released or carried by a candidate"
                    .into(),
            ),
            _ => return Err(format!("unknown artifact kind {}", a.kind)),
        };
        if a.layer != layer {
            return Err(format!("{}: incorrect layer", a.name));
        }
        if matches!(a.kind.as_str(), "npm" | "binary") {
            if a.slsa_build_level != Some(2) {
                return Err(format!("{}: only SLSA build level 2 is supported", a.name));
            }
            if !m
                .artifacts
                .iter()
                .any(|s| Some(&s.name) == a.sbom.as_ref() && s.kind == "sbom")
            {
                return Err(format!("{}: missing SBOM", a.name));
            }
        }
        if a.kind == "npm"
            && (a
                .package
                .as_ref()
                .is_none_or(|p| !p.strip_prefix(crate::npm::SCOPE).is_some_and(plain))
                || a.integrity
                    .as_ref()
                    .is_none_or(|i| !i.starts_with("sha512-")))
        {
            return Err(format!("{}: missing npm identity or integrity", a.name));
        }
    }
    Ok(())
}

fn artifact(kind: &str, name: String, bytes: &[u8]) -> WorkflowArtifact {
    WorkflowArtifact {
        kind: kind.into(),
        name,
        layer: match kind {
            "npm" => 6,
            "binary" => 7,
            _ => 8,
        },
        bytes: bytes.len() as u64,
        sha256: sha256_hex(bytes),
        package: None,
        integrity: None,
        sbom: None,
        slsa_build_level: None,
    }
}

/// Spec 031 3.9: only the three downloaded workflow bundles and their
/// deterministic checksum sidecars are admitted. Links are refused.
pub fn workflow_files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    crate::artifacts::files(dir)
}

fn path(a: &WorkflowArtifact) -> String {
    format!(
        "{}/{}",
        match a.kind.as_str() {
            "npm" => "npm",
            "binary" => "cli",
            _ => "sbom",
        },
        a.name
    )
}

/// Spec 031 3.11.2: candidate evidence also supplies crate SBOMs and notes.
pub fn collect(
    set: &ReleaseSet,
    evidence: &Path,
    artifacts: Option<&Path>,
) -> Result<Vec<WorkflowArtifact>, String> {
    let mut result = Vec::new();
    let mut rest = Vec::new();
    // 058 3.7: a candidate uses the run's artifact SBOMs, not crate
    // packaging or release changelog evidence. Its notes state its limitations.
    let candidate = crate::candidate::is_candidate_version(&set.version);
    if candidate && artifacts.is_none() {
        return Err("058 3.7: a candidate requires the run's artifacts".into());
    }
    for name in set
        .upload_order
        .iter()
        .map(|n| format!("sbom-{n}.cdx.json"))
        .chain(["changelog.md".into()])
        .filter(|_| !candidate)
    {
        let b = std::fs::read(evidence.join(&name)).map_err(|e| format!("{name}: {e}"))?;
        rest.push(artifact(
            if name == "changelog.md" {
                "changelog"
            } else {
                "sbom"
            },
            name,
            &b,
        ));
    }
    if let Some(dir) = artifacts {
        let files = workflow_files(dir)?;
        let mut expected = Vec::new();
        for n in &set.npm {
            expected.push((
                "npm",
                format!(
                    "{}-{}.tgz",
                    n.name.trim_start_matches('@').replace('/', "-"),
                    set.version
                ),
                Some(n.name.clone()),
            ));
        }
        if !set
            .absent
            .iter()
            .any(|a| a.kind == "binary" && a.name == "rustev")
        {
            for p in PLATFORMS {
                expected.push(("binary", format!("rustev-{}-{p}.tar.gz", set.version), None));
            }
        }
        for (kind, name, package) in expected {
            let key = format!("{}/{name}", if kind == "npm" { "npm" } else { "cli" });
            let bytes = files.get(&key).ok_or_else(|| format!("missing {key}"))?;
            let sbom = format!("{name}.cdx.json");
            let sb = files
                .get(&format!("sbom/{sbom}"))
                .ok_or_else(|| format!("{name}: missing SBOM {sbom}"))?;
            let value: serde_json::Value =
                serde_json::from_slice(sb).map_err(|e| format!("{sbom}: {e}"))?;
            if value["bomFormat"] != "CycloneDX" || value["specVersion"] != "1.5" {
                return Err(format!("{sbom}: expected CycloneDX 1.5"));
            }
            let mut a = artifact(kind, name, bytes);
            a.package = package;
            a.integrity = (kind == "npm").then(|| integrity(bytes));
            a.sbom = Some(sbom.clone());
            a.slsa_build_level = Some(2);
            result.push(a);
            rest.push(artifact("sbom", sbom, sb));
        }
        let mut m = result.clone();
        m.extend(
            rest.iter()
                .filter(|a| {
                    a.name.ends_with(".tgz.cdx.json") || a.name.ends_with(".tar.gz.cdx.json")
                })
                .cloned(),
        );
        let errors = compare_files(&m, &files);
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
    }
    rest.sort_by(|a, b| a.name.cmp(&b.name));
    result.extend(rest);
    Ok(result)
}

fn compare_files(artifacts: &[WorkflowArtifact], files: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    let mut errors = Vec::new();
    let mut expected = BTreeSet::new();
    for a in artifacts {
        let p = path(a);
        expected.insert(p.clone());
        match files.get(&p) {
            None => errors.push(format!("missing {p}")),
            Some(b)
                if b.len() as u64 != a.bytes
                    || sha256_hex(b) != a.sha256
                    || (a.kind == "npm" && a.integrity.as_ref() != Some(&integrity(b))) =>
            {
                errors.push(format!("{p}: bytes differ from manifest"))
            }
            _ => {}
        }
        if a.kind == "binary" {
            let sidecar = format!("{p}.sha256");
            expected.insert(sidecar.clone());
            let want = format!("{}  {}\n", a.sha256, a.name);
            if files.get(&sidecar).is_none_or(|b| *b != want.as_bytes()) {
                errors.push(format!("{sidecar}: missing or differs"));
            }
        }
    }
    // artifacts.sh pack writes this derived file; verify its contents too.
    if files.contains_key("npm/SHA256SUMS") {
        expected.insert("npm/SHA256SUMS".into());
        let mut npm: Vec<_> = artifacts.iter().filter(|a| a.kind == "npm").collect();
        npm.sort_by(|a, b| a.name.cmp(&b.name));
        let want: String = npm
            .iter()
            .map(|a| format!("{}  ./{}\n", a.sha256, a.name))
            .collect();
        if files["npm/SHA256SUMS"] != want.as_bytes() {
            errors.push("npm/SHA256SUMS: differs".into());
        }
    }
    for p in files.keys() {
        if !expected.contains(p) {
            errors.push(format!("unexpected {p}"));
        }
    }
    errors
}

/// Spec 031 3.9.3: compare all rehearsal artifacts, reporting every difference.
pub fn verify(m: &ReleaseManifest, dir: &Path) -> Vec<String> {
    if let Err(e) = validate(m) {
        return vec![e];
    }
    let workflow: Vec<_> = m
        .artifacts
        .iter()
        .filter(|a| {
            a.kind == "npm"
                || a.kind == "binary"
                || a.name.ends_with(".tgz.cdx.json")
                || a.name.ends_with(".tar.gz.cdx.json")
        })
        .cloned()
        .collect();
    match workflow_files(dir) {
        Ok(files) => compare_files(&workflow, &files),
        Err(e) => vec![e],
    }
}

/// Spec 031 3.11.2: flatten the release assets beside the crates.
pub fn stage(m: &ReleaseManifest, evidence: &Path, artifacts: Option<&Path>) -> Result<(), String> {
    if let Some(dir) = artifacts {
        for a in &m.artifacts {
            let source = dir.join(path(a));
            if source.is_file() {
                std::fs::copy(source, evidence.join(&a.name)).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}
