//! Spec 031 3.1 and 058 3.4.1: the registries' public state for a
//! manifest, read anonymously.
use crate::{
    lifecycle::{Lookup, Registry},
    manifest::ReleaseManifest,
};
use std::process::Command;

/// Spec 031 3.1: collect every missing or mismatched crate, without a token.
pub fn observe_crates(m: &ReleaseManifest, registry: &mut dyn Registry) -> Vec<String> {
    m.packages
        .iter()
        .filter_map(|p| match registry.lookup(&p.name, &m.version) {
            Lookup::Visible { checksum } if checksum == p.sha256 => None,
            Lookup::Visible { .. } => Some(format!("{}: checksum mismatch", p.name)),
            Lookup::Absent => Some(format!("{}: missing", p.name)),
            // `Unattested` is npm's answer only (A-87); crates.io never gives it,
            // and the match must still refuse it rather than take it for present.
            Lookup::Unattested { .. } => Some(format!("{}: not a crates.io answer", p.name)),
            Lookup::Unknown { detail } => Some(format!(
                "{}: unknown ({})",
                p.name,
                crate::lifecycle::redact(&detail)
            )),
        })
        .collect()
}

/// Spec 031 3.1: npm dist.integrity is the public visibility checksum, and
/// (A-87) a version counts as `Visible` only with an SLSA build-provenance
/// attestation; one without it is `Unattested`.
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
        "200" => {
            let v = serde_json::from_str::<serde_json::Value>(body).unwrap_or_default();
            match v["dist"]["integrity"].as_str().map(str::to_string) {
                Some(checksum) if npm_has_provenance(&v) => Lookup::Visible { checksum },
                Some(checksum) => Lookup::Unattested { checksum },
                None => Lookup::Unknown {
                    detail: "npm response lacks integrity".into(),
                },
            }
        }
        _ => Lookup::Unknown {
            detail: format!("npm HTTP {code}"),
        },
    }
}

/// A-87: the version document names an SLSA build-provenance attestation,
/// which npm records only for a `--provenance` publish from a supported CI.
fn npm_has_provenance(v: &serde_json::Value) -> bool {
    let a = &v["dist"]["attestations"];
    a["url"].as_str().is_some_and(|u| !u.is_empty())
        && a["provenance"]["predicateType"]
            .as_str()
            .is_some_and(|t| t.starts_with("https://slsa.dev/provenance/"))
}
