//! The npm members of the release family (spec 031 3.1, the npm layer that
//! follows the last crate layer) and the artifacts the release-set file
//! lists as absent.
//!
//! [`check_npm`] judges each npm member's `package.json` against the release
//! set: its name, the family version (lockstep, 3.1), the license and the
//! repository directory it is built from, the `private` guard spec 027
//! keeps until publication, and, for the native main package, an
//! `optionalDependencies` map naming exactly the per-platform members at the
//! exact family version, each uploaded before it. Nothing here packs,
//! uploads or reads a credential.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

use crate::{LICENSE, REPOSITORY, ReleaseSet};

/// The npm scope of every member (spec 031, Decision 1).
pub const SCOPE: &str = "@statecrafting/";
/// The native main package; its per-platform packages are
/// `@statecrafting/rustev-<platform>` under `integrations/rustev-node/npm/`.
pub const NATIVE_MAIN: &str = "@statecrafting/rustev";
/// The WebAssembly package.
pub const WASM: &str = "@statecrafting/rustev-wasm";
/// The artifact kinds of spec 031 3.1 an absence may name.
pub const ABSENT_KINDS: &[&str] = &["crate", "npm", "binary", "image"];

/// One npm member: its package name and the repository directory holding
/// its `package.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpmPackage {
    pub name: String,
    pub directory: String,
}

/// A planned family member not in this release, with the reason (3.1).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Absent {
    pub kind: String,
    pub name: String,
    pub reason: String,
}

/// One broken npm rule, named by the spec 031 clause it breaks.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum NpmViolation {
    /// 3.1: the npm list or the absences are not well formed.
    Set { detail: String },
    /// 3.1: a member's `package.json` is missing or unreadable.
    Unreadable { name: String, detail: String },
    /// 3.1: a field of a member's `package.json` is wrong.
    Field {
        name: String,
        field: String,
        detail: String,
    },
}

impl std::fmt::Display for NpmViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NpmViolation::Set { detail } => write!(f, "031 3.1: release set: {detail}"),
            NpmViolation::Unreadable { name, detail } => {
                write!(f, "031 3.1: npm member `{name}`: {detail}")
            }
            NpmViolation::Field {
                name,
                field,
                detail,
            } => write!(f, "031 3.1: npm member `{name}` {field}: {detail}"),
        }
    }
}

/// The platform suffix of a per-platform native package, if `name` is one.
pub fn platform_of(name: &str) -> Option<&str> {
    name.strip_prefix(NATIVE_MAIN)
        .and_then(|s| s.strip_prefix('-'))
        .filter(|s| *s != "wasm" && !s.is_empty())
}

/// The one repository directory a family npm package is built from, or
/// `None` for a name that is not a family package. A platform name is
/// lowercase ASCII letters, digits and single hyphens, so no name can reach
/// outside `integrations/rustev-node/npm/`.
pub fn expected_directory(name: &str) -> Option<String> {
    if name == NATIVE_MAIN {
        return Some("integrations/rustev-node".into());
    }
    if name == WASM {
        return Some("integrations/rustev-wasm".into());
    }
    let platform = platform_of(name)?;
    let plain = platform.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    plain.then(|| format!("integrations/rustev-node/npm/{platform}"))
}

/// The npm list and the absences, independent of the repository.
pub fn check_npm_set(set: &ReleaseSet) -> Vec<NpmViolation> {
    let mut v = Vec::new();
    let mut bad = |detail: String| v.push(NpmViolation::Set { detail });
    let mut seen = BTreeSet::new();
    for p in &set.npm {
        if !seen.insert(p.name.as_str()) {
            bad(format!("npm member `{}` is listed twice", p.name));
        }
        if !p.name.starts_with(SCOPE) {
            bad(format!(
                "npm member `{}` is outside the {SCOPE} scope",
                p.name
            ));
        }
        let Some(expected) = expected_directory(&p.name) else {
            bad(format!("npm member `{}` is not a family package", p.name));
            continue;
        };
        if p.directory != expected {
            bad(format!(
                "npm member `{}` is built from {}, not {expected}",
                p.name, p.directory
            ));
        }
    }
    // The npm layer's order (3.1): each per-platform package, then the
    // native main package naming them, then the WebAssembly package.
    let position = |n: &str| set.npm.iter().position(|p| p.name == n);
    let main = position(NATIVE_MAIN);
    for (i, p) in set.npm.iter().enumerate() {
        if platform_of(&p.name).is_some() && main.is_some_and(|m| i > m) {
            bad(format!("`{}` uploads after {NATIVE_MAIN}", p.name));
        }
    }
    let platforms: Vec<&str> = set
        .npm
        .iter()
        .filter_map(|p| platform_of(&p.name))
        .collect();
    if platforms.windows(2).any(|w| w[0] >= w[1]) {
        bad("the per-platform packages are not ordered by name".into());
    }
    if !platforms.is_empty() && main.is_none() {
        bad(format!("per-platform packages without {NATIVE_MAIN}"));
    }
    if position(WASM).is_some_and(|w| w + 1 != set.npm.len()) {
        bad(format!("{WASM} does not upload last of the npm members"));
    }

    let members: BTreeSet<&str> = set
        .members()
        .chain(set.npm.iter().map(|p| p.name.as_str()))
        .collect();
    let mut absent = BTreeSet::new();
    for a in &set.absent {
        if !ABSENT_KINDS.contains(&a.kind.as_str()) {
            bad(format!(
                "absence `{}` has the unknown kind `{}`",
                a.name, a.kind
            ));
        }
        if a.reason.trim().is_empty() {
            bad(format!("the absence of `{}` has no reason", a.name));
        }
        if !absent.insert((a.kind.as_str(), a.name.as_str())) {
            bad(format!("absence `{}` is listed twice", a.name));
        }
        // A crate or npm package cannot be both shipped and absent. An
        // absent prebuilt binary keeps the name of the crate's binary target
        // (`binaries`), which ships inside the crate, so it is not compared.
        let shipped =
            matches!(a.kind.as_str(), "crate" | "npm") && members.contains(a.name.as_str());
        if shipped {
            bad(format!("`{}` is both a member and absent", a.name));
        }
    }
    v
}

fn text<'a>(doc: &'a Value, key: &str) -> Option<&'a str> {
    doc.get(key).and_then(Value::as_str)
}

/// Judge each npm member's `package.json`, read by `read` from its
/// directory, against the release set. An empty result is the passing
/// verdict.
pub fn check_npm(
    set: &ReleaseSet,
    read: impl Fn(&str) -> Result<Vec<u8>, String>,
) -> Vec<NpmViolation> {
    let mut v = check_npm_set(set);
    let platforms: BTreeSet<&str> = set
        .npm
        .iter()
        .filter(|p| platform_of(&p.name).is_some())
        .map(|p| p.name.as_str())
        .collect();
    for p in &set.npm {
        // Only a member's own directory is read: one the set's shape refused
        // (reported above) is never opened, so no entry reaches another path.
        if expected_directory(&p.name).as_deref() != Some(p.directory.as_str()) {
            continue;
        }
        let path = format!("{}/package.json", p.directory);
        let doc: Value = match read(&path).and_then(|b| {
            serde_json::from_slice(&b).map_err(|e| format!("{path} is not JSON: {e}"))
        }) {
            Ok(d) => d,
            Err(detail) => {
                v.push(NpmViolation::Unreadable {
                    name: p.name.clone(),
                    detail,
                });
                continue;
            }
        };
        let mut field = |field: &str, detail: String| {
            v.push(NpmViolation::Field {
                name: p.name.clone(),
                field: field.into(),
                detail,
            })
        };
        if text(&doc, "name") != Some(p.name.as_str()) {
            field("name", format!("is {:?}", doc.get("name")));
        }
        if text(&doc, "version") != Some(set.version.as_str()) {
            field(
                "version",
                format!(
                    "is {:?}, not the family version {}",
                    doc.get("version"),
                    set.version
                ),
            );
        }
        if text(&doc, "license") != Some(LICENSE) {
            field(
                "license",
                format!("is {:?}, not {LICENSE}", doc.get("license")),
            );
        }
        if text(&doc, "description").is_none_or(|d| d.trim().is_empty()) {
            field("description", "is missing".into());
        }
        let repo = doc.get("repository");
        let url = format!("git+{REPOSITORY}.git");
        if repo.and_then(|r| text(r, "url")) != Some(url.as_str())
            || repo.and_then(|r| text(r, "directory")) != Some(p.directory.as_str())
        {
            field(
                "repository",
                format!("must be {url} with directory {}", p.directory),
            );
        }
        // Spec 027 keeps every package `"private": true` in the repository
        // until publication, so no build of `main` can be published by
        // accident; the release's packing step removes exactly that one
        // field (031 3.12), and nothing else of the checked manifest changes.
        if doc.get("private") != Some(&Value::Bool(true)) {
            field(
                "private",
                "must stay true in the repository until publication (spec 027)".into(),
            );
        }
        match doc.get("files").and_then(Value::as_array) {
            Some(f) if !f.is_empty() => {}
            _ => field("files", "must list what the tarball contains".into()),
        }
        if let Some(platform) = platform_of(&p.name) {
            for key in ["os", "cpu"] {
                if doc
                    .get(key)
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty)
                {
                    field(key, format!("is missing for platform {platform}"));
                }
            }
        }
        let optional: BTreeMap<&str, Option<&str>> = doc
            .get("optionalDependencies")
            .and_then(Value::as_object)
            .map(|m| m.iter().map(|(k, x)| (k.as_str(), x.as_str())).collect())
            .unwrap_or_default();
        if p.name == NATIVE_MAIN {
            let named: BTreeSet<&str> = optional.keys().copied().collect();
            if named != platforms {
                field(
                    "optionalDependencies",
                    format!("names {named:?}, not the release's platform packages {platforms:?}"),
                );
            }
            for (k, req) in &optional {
                if *req != Some(set.version.as_str()) {
                    field(
                        "optionalDependencies",
                        format!("`{k}` is {req:?}, not exactly {}", set.version),
                    );
                }
            }
        } else if !optional.is_empty() {
            field(
                "optionalDependencies",
                "only the native main package has them".into(),
            );
        }
        // Maps of name to range, except the bundled list, which npm takes
        // as an array of names (or `true` for every dependency) under
        // either spelling; an empty map or list declares nothing.
        for key in [
            "dependencies",
            "peerDependencies",
            "bundleDependencies",
            "bundledDependencies",
        ] {
            let declares = |d: &Value| match d {
                Value::Object(m) => !m.is_empty(),
                Value::Array(a) => !a.is_empty(),
                Value::Bool(b) => *b,
                _ => true,
            };
            if doc.get(key).is_some_and(declares) {
                field(
                    key,
                    "the family's npm packages take no runtime dependency".into(),
                );
            }
        }
    }
    v
}
