//! Release-candidate checks of spec 021 (packaging and first public release).
//!
//! The release set (`release-set.json`, 3.1 and section 2) fixes the
//! package names, the family version, the dependency layers, the upload order
//! and the exclusions. [`check_manifests`] judges the workspace's
//! `cargo metadata --format-version 1` output against it: complete public
//! metadata and explicitly allowed Cargo features (031 3.2.3), the registry
//! restriction and exact-family dependency closure (3.2, 3.4), and the package versus binary
//! naming of the CLI (3.4).
//!
//! Nothing here uploads, tags, signs or reads a registry credential. A
//! passing check is candidate evidence only (spec 021, I-1).

pub mod archive;
pub mod artifacts;
pub mod audit;
pub mod changelog;
pub mod cratesio;
pub mod family_artifacts;
pub mod family_lifecycle;
pub mod lifecycle;
pub mod manifest;
pub mod npm;
pub mod repro;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Deserialize;

/// The release-set document (`rustev.release-set/1`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseSet {
    pub schema: String,
    pub version: String,
    pub tag: String,
    pub registry: String,
    pub index: String,
    pub layers: Vec<Vec<String>>,
    pub upload_order: Vec<String>,
    pub binaries: Vec<Binary>,
    /// Spec 031 3.2.3: features authorized by a member's own spec.
    #[serde(default)]
    pub member_features: Vec<MemberFeatures>,
    /// Spec 031 3.1: the npm members, in the npm layer's upload order.
    pub npm: Vec<npm::NpmPackage>,
    /// Spec 031 3.1: planned members not in this release, each with why.
    pub absent: Vec<npm::Absent>,
    pub excluded: Vec<Excluded>,
}

/// Spec 031 3.2.3 and 028 3.2.3: a reviewed non-facade feature allowance.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberFeatures {
    pub package: String,
    pub features: Vec<String>,
    pub spec: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binary {
    pub package: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Excluded {
    pub name: String,
    pub reason: String,
}

pub const RELEASE_SET_SCHEMA: &str = "rustev.release-set/1";
/// The source string Cargo reports for a crates.io package.
pub const CRATES_IO_INDEX: &str = "registry+https://github.com/rust-lang/crates.io-index";
pub const REPOSITORY: &str = "https://github.com/statecrafting/rustev";
pub const LICENSE: &str = "Apache-2.0";
/// The facade crate (spec 031, 3.2): the member with one feature per optional slice,
/// always alone in the last crate layer (3.1).
pub const FACADE: &str = "rustev";

impl ReleaseSet {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes).map_err(|e| format!("unreadable release set: {e}"))
    }

    /// Every member, in layer order.
    pub fn members(&self) -> impl Iterator<Item = &str> {
        self.layers.iter().flatten().map(String::as_str)
    }

    /// The one-based layer of `name`, if it is a member.
    pub fn layer_of(&self, name: &str) -> Option<usize> {
        self.layers
            .iter()
            .position(|l| l.iter().any(|m| m == name))
            .map(|i| i + 1)
    }
}

/// The subset of `cargo metadata --format-version 1` these checks read.
#[derive(Debug, Clone, Deserialize)]
pub struct Metadata {
    pub packages: Vec<Package>,
    pub workspace_members: Vec<String>,
    /// Present unless `--no-deps`; the audit walks it (3.6).
    #[serde(default)]
    pub resolve: Option<audit::Resolve>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub version: String,
    pub source: Option<String>,
    pub publish: Option<Vec<String>>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub documentation: Option<String>,
    pub readme: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    pub edition: String,
    pub rust_version: Option<String>,
    #[serde(default)]
    pub features: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub targets: Vec<Target>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Target {
    pub name: String,
    pub kind: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub req: String,
    pub kind: Option<String>,
    pub source: Option<String>,
    pub path: Option<String>,
    pub registry: Option<String>,
    #[serde(default)]
    pub optional: bool,
}

impl Dependency {
    /// `normal`, `build` or `dev`.
    pub fn kind_name(&self) -> &str {
        self.kind.as_deref().unwrap_or("normal")
    }
}

/// One broken rule, named by the spec 021 clause it breaks.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Violation {
    /// Section 2 and 3.2: the release set is not one well-formed family.
    ReleaseSet { detail: String },
    /// Section 2: a member is not a workspace package.
    MissingMember { name: String },
    /// Section 2: a workspace package is neither a member nor an exclusion.
    Unlisted { name: String },
    /// 3.2: a member's version is not the family version.
    Version { name: String, found: String },
    /// 3.4: a member may publish elsewhere, or an exclusion may publish.
    Publish { name: String, found: String },
    /// 3.3: a public metadata field is missing or wrong.
    Metadata {
        name: String,
        field: String,
        detail: String,
    },
    /// 3.3: the first release declares no Cargo feature (the facade's
    /// excepted, spec 031 3.2).
    Feature { name: String, feature: String },
    /// Spec 031 3.2 and I-3: a facade feature that is not exactly one
    /// optional dependency on a release member, or a non-empty default.
    FacadeFeature { feature: String, detail: String },
    /// 3.2 and 3.4: an internal dependency is not a member at the exact
    /// family version.
    InternalDependency {
        name: String,
        dependency: String,
        kind: String,
        detail: String,
    },
    /// 3.4: a dependency or resolved package comes from a Git source, an
    /// alternate registry or a path outside the workspace.
    Source { name: String, detail: String },
    /// Section 2: a member's layer is not one past its deepest normal or
    /// build dependency.
    Layer {
        name: String,
        declared: usize,
        expected: usize,
    },
    /// 3.9: a member uploads before a dependency of any kind.
    Order { name: String, dependency: String },
    /// 3.4: the binary targets of the members are not the declared ones.
    Binary { detail: String },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Violation::ReleaseSet { detail } => write!(f, "021 2: release set: {detail}"),
            Violation::MissingMember { name } => {
                write!(
                    f,
                    "021 2: `{name}` is in the release set but not in the workspace"
                )
            }
            Violation::Unlisted { name } => write!(
                f,
                "021 2: workspace package `{name}` is neither a release member nor an exclusion"
            ),
            Violation::Version { name, found } => {
                write!(
                    f,
                    "021 3.2: `{name}` is version {found}, not the family version"
                )
            }
            Violation::Publish { name, found } => {
                write!(f, "021 3.4: `{name}` has publish = {found}")
            }
            Violation::Metadata {
                name,
                field,
                detail,
            } => {
                write!(f, "021 3.3: `{name}` {field}: {detail}")
            }
            Violation::Feature { name, feature } => {
                write!(
                    f,
                    "031 3.2.3: `{name}` feature `{feature}` differs from its allowance or has a nonempty default"
                )
            }
            Violation::FacadeFeature { feature, detail } => {
                write!(f, "031 3.2: the facade's feature `{feature}` {detail}")
            }
            Violation::InternalDependency {
                name,
                dependency,
                kind,
                detail,
            } => write!(
                f,
                "021 3.2: `{name}` {kind} dependency `{dependency}`: {detail}"
            ),
            Violation::Source { name, detail } => write!(f, "021 3.4: `{name}`: {detail}"),
            Violation::Layer {
                name,
                declared,
                expected,
            } => write!(
                f,
                "021 2: `{name}` is in layer {declared}; its normal and build dependencies put it in layer {expected}"
            ),
            Violation::Order { name, dependency } => write!(
                f,
                "021 3.9: `{name}` uploads before its dependency `{dependency}`"
            ),
            Violation::Binary { detail } => write!(f, "021 3.4: {detail}"),
        }
    }
}

/// The release set's own shape, independent of the workspace.
pub fn check_set(set: &ReleaseSet) -> Vec<Violation> {
    let mut v = Vec::new();
    let mut bad = |detail: String| v.push(Violation::ReleaseSet { detail });
    if set.schema != RELEASE_SET_SCHEMA {
        bad(format!(
            "schema is {:?}, not {RELEASE_SET_SCHEMA:?}",
            set.schema
        ));
    }
    let numeric = set.version.split('.').collect::<Vec<_>>();
    if numeric.len() != 3
        || numeric.iter().any(|p| {
            p.is_empty()
                || !p.bytes().all(|b| b.is_ascii_digit())
                || (p.len() > 1 && p.starts_with('0'))
        })
    {
        bad(format!("version {:?} is not X.Y.Z", set.version));
    }
    if set.tag != format!("v{}", set.version) {
        bad(format!("tag {:?} is not exactly v{}", set.tag, set.version));
    }
    if set.registry != "crates-io" || set.index != CRATES_IO_INDEX {
        bad("the registry is not crates.io".into());
    }
    if set.layers.is_empty() || set.layers.iter().any(Vec::is_empty) {
        bad("a layer is empty".into());
    }
    let mut seen = BTreeSet::new();
    for m in set.members() {
        if !seen.insert(m) {
            bad(format!("`{m}` is listed twice"));
        }
    }
    for layer in &set.layers {
        if layer.windows(2).any(|w| w[0] >= w[1]) {
            bad(format!("layer {layer:?} is not sorted by name"));
        }
    }
    // Spec 031 3.1: the facade, when a member, is the last crate layer
    // alone and uploads last.
    if seen.contains(FACADE) {
        if set.layers.last().map(Vec::as_slice) != Some(&[FACADE.to_string()][..]) {
            bad(format!(
                "the facade `{FACADE}` is not alone in the last layer"
            ));
        }
        if set.upload_order.last().map(String::as_str) != Some(FACADE) {
            bad(format!("the facade `{FACADE}` does not upload last"));
        }
    }
    let order: BTreeSet<&str> = set.upload_order.iter().map(String::as_str).collect();
    if order != seen || order.len() != set.upload_order.len() {
        bad("upload_order is not a permutation of the members".into());
    }
    // Spec 031 3.2.3: only a member's own spec may add features.
    let mut allowed = BTreeSet::new();
    for allowance in &set.member_features {
        if allowance.package == FACADE || !seen.contains(allowance.package.as_str()) {
            bad(format!(
                "feature allowance for non-member or facade `{}`",
                allowance.package
            ));
        }
        if !allowed.insert(&allowance.package) {
            bad(format!(
                "duplicate feature allowance for `{}`",
                allowance.package
            ));
        }
        let features: BTreeSet<_> = allowance.features.iter().collect();
        if allowance.spec.trim().is_empty()
            || features.len() != allowance.features.len()
            || features.iter().any(|f| f.is_empty() || *f == "default")
        {
            bad(format!(
                "invalid feature allowance for `{}`",
                allowance.package
            ));
        }
    }
    for e in &set.excluded {
        if seen.contains(e.name.as_str()) {
            bad(format!("`{}` is both a member and an exclusion", e.name));
        }
        if e.reason.trim().is_empty() {
            bad(format!("the exclusion of `{}` has no reason", e.name));
        }
    }
    for b in &set.binaries {
        if !seen.contains(b.package.as_str()) {
            bad(format!(
                "binary `{}` belongs to non-member `{}`",
                b.name, b.package
            ));
        }
    }
    v
}

fn publish_text(p: &Option<Vec<String>>) -> String {
    match p {
        None => "unrestricted".into(),
        Some(l) => format!("{l:?}"),
    }
}

fn keyword_ok(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 20
        && k.as_bytes()[0].is_ascii_alphanumeric()
        && k.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The package ids in the release members' closure (3.4): every package a
/// member reaches through a normal, build or development edge, then through
/// normal and build edges from there. `None` when `meta` has no resolve
/// graph. A dependency with no kind listed counts as normal.
pub fn release_closure<'a>(
    meta: &'a Metadata,
    members: &BTreeSet<&str>,
) -> Option<BTreeSet<&'a str>> {
    let resolve = meta.resolve.as_ref()?;
    let nodes: BTreeMap<&str, &audit::Node> =
        resolve.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let is_normal = |d: &audit::NodeDep| {
        d.dep_kinds.is_empty() || d.dep_kinds.iter().any(|k| k.kind.as_deref() != Some("dev"))
    };
    let mut stack: Vec<&str> = Vec::new();
    for p in &meta.packages {
        if !members.contains(p.name.as_str()) || !meta.workspace_members.contains(&p.id) {
            continue;
        }
        if let Some(n) = nodes.get(p.id.as_str()) {
            stack.extend(n.deps.iter().map(|d| d.pkg.as_str()));
        }
    }
    let mut seen = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(n) = nodes.get(id) {
            stack.extend(
                n.deps
                    .iter()
                    .filter(|d| is_normal(d))
                    .map(|d| d.pkg.as_str()),
            );
        }
    }
    Some(seen)
}

/// Judge the workspace's metadata against the release set. An empty result
/// is the passing verdict.
pub fn check_manifests(set: &ReleaseSet, meta: &Metadata) -> Vec<Violation> {
    let mut v = check_set(set);
    let ws: BTreeSet<&str> = meta.workspace_members.iter().map(String::as_str).collect();
    let workspace: BTreeMap<&str, &Package> = meta
        .packages
        .iter()
        .filter(|p| ws.contains(p.id.as_str()))
        .map(|p| (p.name.as_str(), p))
        .collect();
    let members: BTreeSet<&str> = set.members().collect();
    let excluded: BTreeSet<&str> = set.excluded.iter().map(|e| e.name.as_str()).collect();
    let position: BTreeMap<&str, usize> = set
        .upload_order
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();

    for m in &members {
        if !workspace.contains_key(m) {
            v.push(Violation::MissingMember {
                name: m.to_string(),
            });
        }
    }
    for e in &excluded {
        if !workspace.contains_key(e) {
            v.push(Violation::ReleaseSet {
                detail: format!("exclusion `{e}` is not a workspace package"),
            });
        }
    }
    for (name, p) in &workspace {
        if members.contains(name) {
            if p.publish.as_deref() != Some(&[set.registry.clone()][..]) {
                v.push(Violation::Publish {
                    name: name.to_string(),
                    found: publish_text(&p.publish),
                });
            }
        } else if excluded.contains(name) {
            if p.publish.as_deref() != Some(&[][..]) {
                v.push(Violation::Publish {
                    name: name.to_string(),
                    found: publish_text(&p.publish),
                });
            }
        } else {
            v.push(Violation::Unlisted {
                name: name.to_string(),
            });
        }
    }

    // The shared MSRV, read from the first member present, so a missing
    // member reports only `MissingMember` rather than a cascade.
    let rust_version = set
        .upload_order
        .iter()
        .find_map(|n| workspace.get(n.as_str()))
        .and_then(|p| p.rust_version.clone());
    for name in &members {
        let Some(p) = workspace.get(name) else {
            continue;
        };
        check_member(set, p, rust_version.as_deref(), &mut v);
        for d in &p.dependencies {
            if workspace.contains_key(d.name.as_str()) || members.contains(d.name.as_str()) {
                check_internal(set, p, d, &members, &position, &mut v);
            } else {
                check_external(p, d, &mut v);
            }
        }
        // Layers are topological over normal and build dependencies (2).
        let expected = 1 + p
            .dependencies
            .iter()
            .filter(|d| d.kind_name() != "dev")
            .filter_map(|d| set.layer_of(&d.name))
            .max()
            .unwrap_or(0);
        // The facade is always the last crate layer, alone (spec 031 3.1):
        // every optional dependency must already be public.
        let expected = if *name == FACADE {
            expected.max(set.layers.len())
        } else {
            expected
        };
        if let Some(declared) = set.layer_of(name)
            && declared != expected
        {
            v.push(Violation::Layer {
                name: name.to_string(),
                declared,
                expected,
            });
        }
    }

    // Every resolved package outside the workspace in the members' closure
    // comes from crates.io; a path package outside the workspace would be a
    // patch or override. A package only a non-member reaches (an excluded
    // integration's Git dependency) is not judged (3.4, R-49). Without a
    // resolve graph every resolved package is judged, so the check never
    // passes on less evidence.
    let closure = release_closure(meta, &members);
    for p in &meta.packages {
        if ws.contains(p.id.as_str()) {
            continue;
        }
        if closure.as_ref().is_some_and(|c| !c.contains(p.id.as_str())) {
            continue;
        }
        match &p.source {
            Some(s) if s == CRATES_IO_INDEX => {}
            Some(s) => v.push(Violation::Source {
                name: p.name.clone(),
                detail: format!("resolved from {s}, not crates.io"),
            }),
            None => v.push(Violation::Source {
                name: p.name.clone(),
                detail: "resolved from a local path outside the workspace".into(),
            }),
        }
    }

    // Binary names (3.4): exactly the declared binaries among the members.
    let mut found: BTreeSet<(String, String)> = BTreeSet::new();
    for name in &members {
        if let Some(p) = workspace.get(name) {
            for t in p
                .targets
                .iter()
                .filter(|t| t.kind.iter().any(|k| k == "bin"))
            {
                found.insert((p.name.clone(), t.name.clone()));
            }
        }
    }
    let declared: BTreeSet<(String, String)> = set
        .binaries
        .iter()
        .map(|b| (b.package.clone(), b.name.clone()))
        .collect();
    if found != declared {
        v.push(Violation::Binary {
            detail: format!("member binaries are {found:?}, declared {declared:?}"),
        });
    }
    v.sort();
    v.dedup();
    v
}

fn check_member(set: &ReleaseSet, p: &Package, rust_version: Option<&str>, v: &mut Vec<Violation>) {
    let name = p.name.as_str();
    if p.version != set.version {
        v.push(Violation::Version {
            name: name.to_string(),
            found: p.version.clone(),
        });
    }
    let mut meta = |field: &str, detail: String| {
        v.push(Violation::Metadata {
            name: name.to_string(),
            field: field.to_string(),
            detail,
        })
    };
    let nonempty = |s: &Option<String>| s.as_deref().is_some_and(|s| !s.trim().is_empty());
    if !nonempty(&p.description) {
        meta("description", "missing".into());
    }
    if p.license.as_deref() != Some(LICENSE) {
        meta("license", format!("{:?}, not {LICENSE:?}", p.license));
    }
    if p.repository.as_deref() != Some(REPOSITORY) {
        meta(
            "repository",
            format!("{:?}, not {REPOSITORY:?}", p.repository),
        );
    }
    if p.homepage.as_deref() != Some(REPOSITORY) {
        meta("homepage", format!("{:?}, not {REPOSITORY:?}", p.homepage));
    }
    let docs = format!("https://docs.rs/{name}");
    if p.documentation.as_deref() != Some(docs.as_str()) {
        meta(
            "documentation",
            format!("{:?}, not {docs:?}", p.documentation),
        );
    }
    if !nonempty(&p.readme) {
        meta("readme", "missing".into());
    }
    if p.keywords.is_empty() || p.keywords.len() > 5 || !p.keywords.iter().all(|k| keyword_ok(k)) {
        meta(
            "keywords",
            format!("{:?} is not 1 to 5 registry keywords", p.keywords),
        );
    }
    if p.categories.is_empty() || p.categories.len() > 5 {
        meta(
            "categories",
            format!("{:?} is not 1 to 5 categories", p.categories),
        );
    }
    if p.edition != "2024" {
        meta("edition", format!("{}, not 2024", p.edition));
    }
    if p.rust_version.is_none() || p.rust_version.as_deref() != rust_version {
        meta(
            "rust-version",
            format!("{:?}, not the shared {rust_version:?}", p.rust_version),
        );
    }
    if name == FACADE {
        check_facade_features(set, p, v);
    } else {
        // Spec 031 3.2.3; spec 028 3.2.3 authorizes the OTLP features.
        let allowance = set.member_features.iter().find(|a| a.package == name);
        let expected: BTreeSet<&str> = allowance
            .into_iter()
            .flat_map(|a| a.features.iter().map(String::as_str))
            .collect();
        let declared: BTreeSet<&str> = p
            .features
            .keys()
            .map(String::as_str)
            .filter(|f| *f != "default")
            .collect();
        for feature in expected.symmetric_difference(&declared) {
            v.push(Violation::Feature {
                name: name.to_string(),
                feature: (*feature).to_string(),
            });
        }
        if p.features
            .get("default")
            .is_some_and(|default| !default.is_empty() || allowance.is_none())
        {
            v.push(Violation::Feature {
                name: name.to_string(),
                feature: "default".into(),
            });
        }
    }
}

/// Spec 031 3.2 and I-3: no default features, and each feature enables
/// exactly one optional normal dependency that is a member of this release;
/// a feature whose crate is not released is absent, never a stub.
fn check_facade_features(set: &ReleaseSet, p: &Package, v: &mut Vec<Violation>) {
    let mut bad = |feature: &str, detail: String| {
        v.push(Violation::FacadeFeature {
            feature: feature.to_string(),
            detail,
        })
    };
    for (feature, enables) in &p.features {
        if feature == "default" {
            if !enables.is_empty() {
                bad(feature, format!("enables {enables:?}; it must be empty"));
            }
            continue;
        }
        let [one] = enables.as_slice() else {
            bad(
                feature,
                format!("enables {enables:?}, not exactly one dependency"),
            );
            continue;
        };
        let Some(dep) = one.strip_prefix("dep:") else {
            bad(feature, format!("enables {one:?}, not a `dep:` dependency"));
            continue;
        };
        if !set.members().any(|m| m == dep) {
            bad(
                feature,
                format!("enables `{dep}`, which is not a member of this release"),
            );
            // One violation per feature: an unreleased crate is not a
            // dependency either, which this already explains.
            continue;
        }
        let declared = p
            .dependencies
            .iter()
            .any(|d| d.name == dep && d.optional && d.kind_name() == "normal");
        if !declared {
            bad(
                feature,
                format!("enables `{dep}`, which is not an optional normal dependency"),
            );
        }
    }
}

fn check_internal(
    set: &ReleaseSet,
    p: &Package,
    d: &Dependency,
    members: &BTreeSet<&str>,
    position: &BTreeMap<&str, usize>,
    v: &mut Vec<Violation>,
) {
    let mut bad = |detail: String| {
        v.push(Violation::InternalDependency {
            name: p.name.clone(),
            dependency: d.name.clone(),
            kind: d.kind_name().to_string(),
            detail,
        })
    };
    if !members.contains(d.name.as_str()) {
        bad("not a release member, so it cannot enter a published closure".into());
        return;
    }
    let exact = format!("={}", set.version);
    if d.req != exact {
        bad(format!("requires {:?}, not {exact:?}", d.req));
    }
    if d.registry.is_some() {
        bad("names an alternate registry".into());
    }
    if let (Some(a), Some(b)) = (position.get(p.name.as_str()), position.get(d.name.as_str()))
        && b >= a
    {
        v.push(Violation::Order {
            name: p.name.clone(),
            dependency: d.name.clone(),
        });
    }
}

fn check_external(p: &Package, d: &Dependency, v: &mut Vec<Violation>) {
    let detail = if d.path.is_some() {
        Some(format!(
            "depends on `{}` by a path outside the workspace",
            d.name
        ))
    } else if d.registry.is_some() {
        Some(format!(
            "depends on `{}` from an alternate registry",
            d.name
        ))
    } else {
        match &d.source {
            Some(s) if s == CRATES_IO_INDEX => None,
            Some(s) => Some(format!("depends on `{}` from {s}", d.name)),
            None => Some(format!("depends on `{}` with no registry source", d.name)),
        }
    };
    if let Some(detail) = detail {
        v.push(Violation::Source {
            name: p.name.clone(),
            detail,
        });
    }
}
