//! The workspace dependency rules of spec 001, section 3.4, the package
//! rule of spec 007, section 3.3.1, the binding rules of spec 027,
//! section 3.14, and the telemetry exporter rule of spec 028, section 3.12,
//! checked over the output of `cargo metadata --format-version 1`.
//!
//! The forbidden families are a conservative denylist matched by package name
//! (spec 001, 3.5). Passing this check is not a proof that a crate performs no
//! I/O; it proves that no listed family is in the dependency graph.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use serde::Deserialize;

/// Ecosystem families no crate outside `integrations/` may depend on.
pub const ECOSYSTEM_PREFIXES: &[&str] = &["aicortex", "rahi", "statecraft", "spec-spine"];
/// HTTP client and server stacks no crate outside `integrations/` may depend on.
pub const HTTP_STACKS: &[&str] = &[
    "hyper",
    "reqwest",
    "axum",
    "actix-web",
    "warp",
    "tonic",
    "ureq",
    "isahc",
    "surf",
    "h2",
    "http",
    // The pieces of the hyper stack the remote binding uses (spec 009, 3.11).
    "hyper-util",
    "http-body",
    "http-body-util",
];
/// Async runtime executors `rustev-contract` and `rustev-core` may not reach.
pub const EXECUTORS: &[&str] = &[
    "tokio",
    "async-std",
    "smol",
    "futures-executor",
    "async-executor",
];

/// The Node-API family (spec 027, 3.14.1), by exact name.
pub const NAPI_FAMILY: &[&str] = &["napi", "napi-derive", "napi-sys", "napi-build"];

/// Node-API and WebAssembly binding families no crate outside
/// `integrations/` may depend on (spec 027, 3.14.1); the first four are
/// [`NAPI_FAMILY`].
pub const BINDING_FAMILIES: &[&str] = &[
    "napi",
    "napi-derive",
    "napi-sys",
    "napi-build",
    "wasm-bindgen",
    "wasm-bindgen-macro",
    "wasm-bindgen-futures",
    "js-sys",
    "web-sys",
];
/// What `rustev-wasm`'s normal and build graph may not contain beyond the
/// executors, HTTP stacks and Node-API families (spec 027, 3.14.3).
pub const WASM_FORBIDDEN: &[&str] = &[
    "rustev-runtime",
    "getrandom",
    "web-sys",
    "wasm-bindgen-futures",
];
/// The WebAssembly binding crate.
pub const WASM_CRATE: &str = "rustev-wasm";

/// The telemetry exporter family (spec 028, 3.12.1) is every package whose
/// name starts with this prefix (`opentelemetry_sdk`, `opentelemetry-otlp`,
/// `opentelemetry-proto`, the appenders and the rest), plus
/// [`TELEMETRY_NAMES`].
pub const TELEMETRY_PREFIX: &str = "opentelemetry";
/// Members of the telemetry exporter family named exactly (spec 028, 3.12.1).
pub const TELEMETRY_NAMES: &[&str] = &["tracing-opentelemetry"];

/// The crates whose whole normal and build dependency graph is checked; the
/// shared command surface joins the contract and core (spec 027, 3.14.2).
pub const PURE_CRATES: &[&str] = &["rustev-contract", "rustev-core", "rustev-surface"];
/// The only normal or build dependencies a crate under `packages/` may have
/// (design section 15; spec 007, 3.3.1).
pub const PACKAGE_DEPENDENCIES: &[&str] = &["rustev-contract", "rustev-core"];

/// The facade crate (spec 031, 3.2): the one crate that may depend on a
/// crate under `integrations/`, and only optionally (3.3).
pub const FACADE: &str = "rustev";

/// The subset of `cargo metadata` output this check reads.
#[derive(Debug, Deserialize)]
pub struct Metadata {
    pub packages: Vec<Package>,
    pub workspace_members: Vec<String>,
    pub workspace_root: String,
    pub resolve: Option<Resolve>,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub manifest_path: String,
    pub dependencies: Vec<Dependency>,
    /// The package's Cargo features, each with what it enables.
    #[serde(default)]
    pub features: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub kind: Option<String>,
    pub path: Option<String>,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Deserialize)]
pub struct Resolve {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Deserialize)]
pub struct Node {
    pub id: String,
    pub deps: Vec<NodeDep>,
}

#[derive(Debug, Deserialize)]
pub struct NodeDep {
    pub pkg: String,
    pub dep_kinds: Vec<DepKind>,
}

#[derive(Debug, Deserialize)]
pub struct DepKind {
    pub kind: Option<String>,
}

/// One broken rule, named by the spec 001 clause it breaks.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Violation {
    /// 3.4.2: a crate outside `integrations/` depends on an ecosystem or HTTP family.
    ForbiddenFamily { krate: String, dependency: String },
    /// 3.4.1: a pure crate reaches a forbidden family through its graph.
    ForbiddenTransitive { krate: String, path: Vec<String> },
    /// 3.4.4: a crate depends on a crate under `integrations/`.
    DependsOnIntegration { krate: String, dependency: String },
    /// Spec 031 3.3.1: the facade depends on an integration crate other
    /// than as an optional normal dependency enabled by exactly one feature.
    FacadeIntegration { dependency: String, detail: String },
    /// Spec 031 3.3.2: a workspace crate depends on the facade.
    DependsOnFacade { krate: String },
    /// Spec 031 3.3.3: the facade's `default` feature enables something.
    FacadeDefault { enables: Vec<String> },
    /// Spec 031 3.3 and 3.13: the facade declares a direct dependency on an
    /// ecosystem, HTTP or binding family; it reaches them only through the
    /// integration crates it re-exports.
    FacadeFamily { dependency: String },
    /// 3.4.5: `rustev-contract` depends on a workspace crate, or `rustev-core`
    /// on one other than `rustev-contract`.
    WorkspaceEdge { krate: String, dependency: String },
    /// Spec 007 3.3.1 (design section 15): a crate under `packages/` has a
    /// normal or build dependency other than `rustev-contract` and
    /// `rustev-core`.
    PackageDependency { krate: String, dependency: String },
    /// Spec 027 3.14.3: `rustev-wasm`'s graph reaches the runtime, an
    /// executor, an HTTP stack, a Node-API crate, `getrandom`, `web-sys` or
    /// `wasm-bindgen-futures`.
    WasmGraph { path: Vec<String> },
    /// Spec 028 3.12.2: a crate outside `integrations/` reaches the
    /// telemetry exporter family, directly or through its normal or build
    /// graph.
    TelemetryOutsideIntegrations { krate: String, path: Vec<String> },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Violation::ForbiddenFamily { krate, dependency } => write!(
                f,
                "001 3.4.2: `{krate}` is outside integrations/ and depends on `{dependency}`"
            ),
            Violation::ForbiddenTransitive { krate, path } => write!(
                f,
                "001 3.4.1: `{krate}` reaches a forbidden family: {}",
                path.join(" -> ")
            ),
            Violation::DependsOnIntegration { krate, dependency } => write!(
                f,
                "001 3.4.4: `{krate}` depends on the integration `{dependency}`"
            ),
            Violation::FacadeIntegration { dependency, detail } => write!(
                f,
                "031 3.3.1: the facade `{FACADE}` depends on the integration `{dependency}` {detail}"
            ),
            Violation::DependsOnFacade { krate } => write!(
                f,
                "031 3.3.2: `{krate}` depends on the facade `{FACADE}`; no workspace crate may"
            ),
            Violation::FacadeDefault { enables } => write!(
                f,
                "031 3.3.3: the facade's default features must be empty, not {enables:?}"
            ),
            Violation::FacadeFamily { dependency } => write!(
                f,
                "031 3.3: the facade `{FACADE}` declares `{dependency}` directly; it reaches a family only through an integration crate"
            ),
            Violation::WorkspaceEdge { krate, dependency } => write!(
                f,
                "001 3.4.5: `{krate}` may not depend on the workspace crate `{dependency}`"
            ),
            Violation::PackageDependency { krate, dependency } => write!(
                f,
                "007 3.3.1 package rule: `{krate}` is a package and may depend only on rustev-contract and rustev-core, not `{dependency}`"
            ),
            Violation::WasmGraph { path } => write!(
                f,
                "027 3.14.3: `{WASM_CRATE}` reaches a forbidden crate: {}",
                path.join(" -> ")
            ),
            Violation::TelemetryOutsideIntegrations { krate, path } => write!(
                f,
                "028 3.12.2: `{krate}` is outside integrations/ and reaches the telemetry exporter family: {}",
                path.join(" -> ")
            ),
        }
    }
}

fn is_ecosystem(name: &str) -> bool {
    ECOSYSTEM_PREFIXES.iter().any(|p| name.starts_with(p))
}

fn is_http(name: &str) -> bool {
    HTTP_STACKS.contains(&name)
}

fn is_executor(name: &str) -> bool {
    EXECUTORS.contains(&name)
}

fn is_binding(name: &str) -> bool {
    BINDING_FAMILIES.contains(&name)
}

fn is_napi(name: &str) -> bool {
    NAPI_FAMILY.contains(&name)
}

/// The telemetry exporter family (spec 028, 3.12.1).
fn is_telemetry(name: &str) -> bool {
    name.starts_with(TELEMETRY_PREFIX) || TELEMETRY_NAMES.contains(&name)
}

/// Forbidden in the pure crates' graphs (spec 001 3.4.1, spec 027 3.14.2).
fn forbidden_for_pure(name: &str) -> bool {
    is_ecosystem(name) || is_http(name) || is_executor(name) || is_binding(name)
}

/// Forbidden in `rustev-wasm`'s graph (spec 027, 3.14.3).
fn forbidden_for_wasm(name: &str) -> bool {
    is_executor(name) || is_http(name) || is_napi(name) || WASM_FORBIDDEN.contains(&name)
}

/// Where a workspace member lives, relative to the workspace root.
fn relative_dir(root: &str, manifest_path: &str) -> String {
    let manifest = manifest_path.replace('\\', "/");
    let root = root.replace('\\', "/");
    let rel = manifest
        .strip_prefix(&root)
        .unwrap_or(&manifest)
        .trim_start_matches('/');
    rel.strip_suffix("Cargo.toml")
        .unwrap_or(rel)
        .trim_end_matches('/')
        .to_string()
}

fn under_integrations(rel: &str) -> bool {
    rel == "integrations" || rel.starts_with("integrations/")
}

fn under_packages(rel: &str) -> bool {
    rel.starts_with("packages/")
}

/// Every violation of spec 001 3.4, sorted.
pub fn check(meta: &Metadata) -> Vec<Violation> {
    let mut out = BTreeSet::new();
    let by_id: BTreeMap<&str, &Package> =
        meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();
    let members: Vec<&Package> = meta
        .workspace_members
        .iter()
        .filter_map(|id| by_id.get(id.as_str()).copied())
        .collect();
    let member_names: BTreeSet<&str> = members.iter().map(|p| p.name.as_str()).collect();
    let integration_names: BTreeSet<&str> = members
        .iter()
        .filter(|p| under_integrations(&relative_dir(&meta.workspace_root, &p.manifest_path)))
        .map(|p| p.name.as_str())
        .collect();

    if let Some(facade) = members.iter().find(|p| p.name == FACADE) {
        let enables = facade.features.get("default").cloned().unwrap_or_default();
        if !enables.is_empty() {
            out.insert(Violation::FacadeDefault { enables });
        }
    }

    for pkg in &members {
        let rel = relative_dir(&meta.workspace_root, &pkg.manifest_path);
        let in_integrations = under_integrations(&rel);
        let in_packages = under_packages(&rel);
        for dep in &pkg.dependencies {
            let dep_under_integrations = dep
                .path
                .as_deref()
                .map(|p| {
                    under_integrations(&relative_dir(
                        &meta.workspace_root,
                        &format!("{p}/Cargo.toml"),
                    ))
                })
                .unwrap_or(false);
            // A dependency on the facade is `DependsOnFacade` (031 3.3.2)
            // alone, not also a dependency on an integration.
            if (integration_names.contains(dep.name.as_str()) || dep_under_integrations)
                && dep.name != FACADE
            {
                if pkg.name == FACADE {
                    // 031 3.3.1: optional, normal, and enabled by exactly
                    // one facade feature (`default` names none, 3.3.3).
                    let enabling = pkg
                        .features
                        .iter()
                        .filter(|(f, _)| f.as_str() != "default")
                        .filter(|(_, on)| on.iter().any(|e| *e == format!("dep:{}", dep.name)))
                        .count();
                    let detail = if dep.kind.is_some() {
                        Some(format!(
                            "as a {} dependency",
                            dep.kind.as_deref().unwrap_or("")
                        ))
                    } else if !dep.optional {
                        Some("without `optional = true`".to_string())
                    } else if enabling != 1 {
                        Some(format!("enabled by {enabling} features, not exactly one"))
                    } else {
                        None
                    };
                    if let Some(detail) = detail {
                        out.insert(Violation::FacadeIntegration {
                            dependency: dep.name.clone(),
                            detail,
                        });
                    }
                } else {
                    out.insert(Violation::DependsOnIntegration {
                        krate: pkg.name.clone(),
                        dependency: dep.name.clone(),
                    });
                }
            }
            if dep.name == FACADE && member_names.contains(FACADE) {
                out.insert(Violation::DependsOnFacade {
                    krate: pkg.name.clone(),
                });
            }
            if pkg.name == FACADE
                && (is_ecosystem(&dep.name) || is_http(&dep.name) || is_binding(&dep.name))
            {
                out.insert(Violation::FacadeFamily {
                    dependency: dep.name.clone(),
                });
            }
            if !in_integrations
                && (is_ecosystem(&dep.name) || is_http(&dep.name) || is_binding(&dep.name))
            {
                out.insert(Violation::ForbiddenFamily {
                    krate: pkg.name.clone(),
                    dependency: dep.name.clone(),
                });
            }
            let is_dev = dep.kind.as_deref() == Some("dev");
            // Spec 028 3.12.2: a declared normal or build dependency, even an
            // optional one the resolved graph leaves out.
            if !in_integrations && !is_dev && is_telemetry(&dep.name) {
                out.insert(Violation::TelemetryOutsideIntegrations {
                    krate: pkg.name.clone(),
                    path: vec![pkg.name.clone(), dep.name.clone()],
                });
            }
            if in_packages && !is_dev && !PACKAGE_DEPENDENCIES.contains(&dep.name.as_str()) {
                out.insert(Violation::PackageDependency {
                    krate: pkg.name.clone(),
                    dependency: dep.name.clone(),
                });
            }
            if member_names.contains(dep.name.as_str()) && !is_dev {
                let allowed = match pkg.name.as_str() {
                    "rustev-contract" => false,
                    "rustev-core" => dep.name == "rustev-contract",
                    _ => true,
                };
                if !allowed {
                    out.insert(Violation::WorkspaceEdge {
                        krate: pkg.name.clone(),
                        dependency: dep.name.clone(),
                    });
                }
            }
        }
    }

    if let Some(resolve) = &meta.resolve {
        let graph: BTreeMap<&str, Vec<&str>> = resolve
            .nodes
            .iter()
            .map(|n| {
                let deps = n
                    .deps
                    .iter()
                    .filter(|d| d.dep_kinds.iter().any(|k| k.kind.as_deref() != Some("dev")))
                    .map(|d| d.pkg.as_str())
                    .collect();
                (n.id.as_str(), deps)
            })
            .collect();
        // Packages are as pure as the core (spec 007, 3.3.2), and so is the
        // facade with no features (spec 031, 3.3.4): `cargo metadata`
        // resolves members with their default features, which for the
        // facade are none, so its optional dependencies are not in the graph.
        for pkg in members.iter().filter(|p| {
            PURE_CRATES.contains(&p.name.as_str())
                || p.name == FACADE
                || under_packages(&relative_dir(&meta.workspace_root, &p.manifest_path))
        }) {
            if let Some(path) = forbidden_path(&graph, &by_id, &pkg.id, forbidden_for_pure) {
                out.insert(Violation::ForbiddenTransitive {
                    krate: pkg.name.clone(),
                    path,
                });
            }
        }
        for pkg in members.iter().filter(|p| p.name == WASM_CRATE) {
            if let Some(path) = forbidden_path(&graph, &by_id, &pkg.id, forbidden_for_wasm) {
                out.insert(Violation::WasmGraph { path });
            }
        }
        // Spec 028 3.12.2: every crate outside `integrations/`, through its
        // normal and build graph.
        for pkg in members
            .iter()
            .filter(|p| !under_integrations(&relative_dir(&meta.workspace_root, &p.manifest_path)))
        {
            if let Some(path) = forbidden_path(&graph, &by_id, &pkg.id, is_telemetry) {
                out.insert(Violation::TelemetryOutsideIntegrations {
                    krate: pkg.name.clone(),
                    path,
                });
            }
        }
    }
    out.into_iter().collect()
}

/// The crate and `cargo tree --target` whose own resolution confirms a
/// transitive violation ([`confirmed`]); `None` for a violation that is not
/// transitive.
pub fn own_graph_of(v: &Violation) -> Option<(&str, &'static str)> {
    match v {
        Violation::ForbiddenTransitive { krate, .. } => Some((krate.as_str(), "all")),
        Violation::WasmGraph { .. } => Some((WASM_CRATE, "wasm32-unknown-unknown")),
        _ => None,
    }
}

/// Spec 001 3.5 (R-45), found with spec 010's first integration:
/// `cargo metadata` resolves features once for the whole workspace, so an
/// integration's graph can switch on an optional edge of a shared crate
/// (for example `crypto-common` to `rand_core`) that a pure crate's own
/// build never takes. A transitive violation therefore stands only when
/// the crate's own resolution, as `cargo tree -p <crate> -e normal,build`
/// lists it (`own_graph`, package names), also holds a forbidden package.
/// Every other violation stands as found.
pub fn confirmed(v: &Violation, own_graph: &[&str]) -> bool {
    match v {
        Violation::ForbiddenTransitive { .. } => own_graph.iter().any(|n| forbidden_for_pure(n)),
        Violation::WasmGraph { .. } => own_graph.iter().any(|n| forbidden_for_wasm(n)),
        _ => true,
    }
}

/// The package names in `cargo tree --prefix none --format {p}` output.
pub fn tree_names(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .collect()
}

/// Breadth-first search for the shortest normal or build path from `start`
/// to a package `forbidden` names; the path lists package names.
fn forbidden_path(
    graph: &BTreeMap<&str, Vec<&str>>,
    by_id: &BTreeMap<&str, &Package>,
    start: &str,
    forbidden: fn(&str) -> bool,
) -> Option<Vec<String>> {
    let name_of = |id: &str| {
        by_id
            .get(id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
    let mut seen: BTreeSet<&str> = BTreeSet::from([start]);
    let mut queue: VecDeque<&str> = VecDeque::from([start]);
    while let Some(id) = queue.pop_front() {
        for &next in graph.get(id).map(Vec::as_slice).unwrap_or(&[]) {
            if !seen.insert(next) {
                continue;
            }
            parent.insert(next, id);
            let name = name_of(next);
            if forbidden(&name) {
                let mut path = vec![name];
                let mut cur = next;
                while let Some(&p) = parent.get(cur) {
                    path.push(name_of(p));
                    cur = p;
                }
                path.reverse();
                return Some(path);
            }
            queue.push_back(next);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `(name, kind, path)` of one declared dependency.
    type Dep<'a> = (&'a str, Option<&'a str>, Option<&'a str>);
    /// `(name, directory, dependencies)` of one workspace member.
    type Member<'a> = (&'a str, &'a str, &'a [Dep<'a>]);

    /// A synthetic workspace: `members` are `(name, dir, deps)` where each dep
    /// is `(name, kind, path)`; `extra` are non-member packages; `edges` are
    /// resolved `(from, to, kind)` edges by name.
    fn meta(
        members: &[Member<'_>],
        extra: &[&str],
        edges: &[(&str, &str, Option<&str>)],
    ) -> Metadata {
        let mut packages = vec![];
        for (name, dir, deps) in members {
            let deps: Vec<_> = deps
                .iter()
                .map(|(n, k, p)| json!({"name": n, "kind": k, "path": p.map(|p| format!("/ws/{p}"))}))
                .collect();
            packages.push(json!({
                "id": format!("{name} 0.1.0"),
                "name": name,
                "manifest_path": format!("/ws/{dir}/Cargo.toml"),
                "dependencies": deps,
            }));
        }
        for name in extra {
            packages.push(json!({
                "id": format!("{name} 0.1.0"),
                "name": name,
                "manifest_path": format!("/registry/{name}/Cargo.toml"),
                "dependencies": [],
            }));
        }
        let mut nodes: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
        for (name, _, _) in members {
            nodes.entry(format!("{name} 0.1.0")).or_default();
        }
        for name in extra {
            nodes.entry(format!("{name} 0.1.0")).or_default();
        }
        for (from, to, kind) in edges {
            nodes
                .entry(format!("{from} 0.1.0"))
                .or_default()
                .push(json!({"pkg": format!("{to} 0.1.0"), "dep_kinds": [{"kind": kind}]}));
        }
        let nodes: Vec<_> = nodes
            .into_iter()
            .map(|(id, deps)| json!({"id": id, "deps": deps}))
            .collect();
        let v = json!({
            "packages": packages,
            "workspace_members": members.iter().map(|(n, _, _)| format!("{n} 0.1.0")).collect::<Vec<_>>(),
            "workspace_root": "/ws",
            "resolve": {"nodes": nodes},
        });
        serde_json::from_value(v).expect("synthetic metadata")
    }

    const CONTRACT: Member<'static> = (
        "rustev-contract",
        "crates/rustev-contract",
        &[("serde", None, None)],
    );
    const CORE: Member<'static> = (
        "rustev-core",
        "crates/rustev-core",
        &[("rustev-contract", None, Some("crates/rustev-contract"))],
    );

    #[test]
    fn a_conforming_workspace_passes() {
        let m = meta(
            &[CONTRACT, CORE],
            &["serde"],
            &[
                ("rustev-contract", "serde", None),
                ("rustev-core", "rustev-contract", None),
            ],
        );
        assert_eq!(check(&m), vec![]);
    }

    #[test]
    fn an_ecosystem_dependency_outside_integrations_fails() {
        let m = meta(
            &[
                CONTRACT,
                (
                    "rustev-runtime",
                    "crates/rustev-runtime",
                    &[("rahi-client", None, None)],
                ),
            ],
            &["serde", "rahi-client"],
            &[],
        );
        assert_eq!(
            check(&m),
            vec![Violation::ForbiddenFamily {
                krate: "rustev-runtime".into(),
                dependency: "rahi-client".into()
            }]
        );
    }

    #[test]
    fn the_same_dependency_inside_integrations_passes() {
        let m = meta(
            &[
                CONTRACT,
                (
                    "rustev-rahi",
                    "integrations/rustev-rahi",
                    &[("rahi-client", None, None), ("axum", None, None)],
                ),
            ],
            &["serde", "rahi-client", "axum"],
            &[],
        );
        assert_eq!(check(&m), vec![]);
    }

    #[test]
    fn an_http_stack_outside_integrations_fails() {
        let m = meta(
            &[
                CONTRACT,
                (
                    "rustev-backend-x",
                    "backends/rustev-backend-x",
                    &[("reqwest", None, None)],
                ),
            ],
            &["serde", "reqwest"],
            &[],
        );
        assert!(
            matches!(&check(&m)[..], [Violation::ForbiddenFamily { dependency, .. }] if dependency == "reqwest")
        );
    }

    /// The remote binding of spec 009 as it is laid out: an HTTP stack and
    /// Tokio under `integrations/`, depending on the contract and core.
    const REMOTE_HTTP: Member<'static> = (
        "rustev-remote-http",
        "integrations/rustev-remote-http",
        &[
            ("rustev-contract", None, Some("crates/rustev-contract")),
            ("rustev-core", None, Some("crates/rustev-core")),
            ("hyper", None, None),
            ("hyper-util", None, None),
            ("http-body-util", None, None),
            ("tokio", None, None),
            ("tokio-rustls", None, None),
        ],
    );

    #[test]
    fn the_remote_binding_under_integrations_passes() {
        let m = meta(
            &[CONTRACT, CORE, REMOTE_HTTP],
            &[
                "serde",
                "hyper",
                "hyper-util",
                "http-body-util",
                "tokio",
                "tokio-rustls",
            ],
            &[
                ("rustev-core", "rustev-contract", None),
                ("rustev-remote-http", "rustev-core", None),
                ("rustev-remote-http", "hyper", None),
                ("rustev-remote-http", "tokio", None),
            ],
        );
        assert_eq!(check(&m), vec![]);
    }

    #[test]
    fn a_crate_outside_integrations_on_the_remote_binding_or_its_stack_fails() {
        // Spec 009, section 6: depending on the HTTP binding fails, and so
        // does reaching for the pieces of its stack directly.
        for (dep, path) in [
            (
                "rustev-remote-http",
                Some("integrations/rustev-remote-http"),
            ),
            ("hyper-util", None),
            ("http-body-util", None),
        ] {
            let deps: &[Dep<'_>] = &[(dep, None, path)];
            let m = meta(
                &[
                    CONTRACT,
                    CORE,
                    REMOTE_HTTP,
                    ("rustev-cli", "crates/rustev-cli", deps),
                ],
                &[
                    "serde",
                    "hyper",
                    "hyper-util",
                    "http-body-util",
                    "tokio",
                    "tokio-rustls",
                ],
                &[],
            );
            let v = check(&m);
            assert!(
                v.iter().any(|x| matches!(x,
                    Violation::DependsOnIntegration { krate, .. }
                    | Violation::ForbiddenFamily { krate, .. } if krate == "rustev-cli")),
                "{dep}: {v:?}"
            );
        }
    }

    #[test]
    fn a_transitive_executor_under_core_fails_with_its_path() {
        let m = meta(
            &[CONTRACT, CORE],
            &["serde", "helper", "tokio"],
            &[
                ("rustev-core", "rustev-contract", None),
                ("rustev-contract", "helper", None),
                ("helper", "tokio", None),
            ],
        );
        let v = check(&m);
        assert!(v.contains(&Violation::ForbiddenTransitive {
            krate: "rustev-core".into(),
            path: vec![
                "rustev-core".into(),
                "rustev-contract".into(),
                "helper".into(),
                "tokio".into()
            ],
        }));
        assert!(v.contains(&Violation::ForbiddenTransitive {
            krate: "rustev-contract".into(),
            path: vec!["rustev-contract".into(), "helper".into(), "tokio".into()],
        }));
    }

    #[test]
    fn a_dev_only_executor_under_core_passes() {
        let m = meta(
            &[
                CONTRACT,
                (
                    "rustev-core",
                    "crates/rustev-core",
                    &[("rustev-contract", None, Some("crates/rustev-contract"))],
                ),
            ],
            &["serde", "tokio"],
            &[
                ("rustev-core", "rustev-contract", None),
                ("rustev-core", "tokio", Some("dev")),
            ],
        );
        assert_eq!(check(&m), vec![]);
    }

    #[test]
    fn a_dependency_on_an_integration_fails() {
        let m = meta(
            &[
                CONTRACT,
                ("rustev-serve", "integrations/rustev-serve", &[]),
                (
                    "rustev-cli",
                    "crates/rustev-cli",
                    &[("rustev-serve", None, Some("integrations/rustev-serve"))],
                ),
            ],
            &["serde"],
            &[],
        );
        assert_eq!(
            check(&m),
            vec![Violation::DependsOnIntegration {
                krate: "rustev-cli".into(),
                dependency: "rustev-serve".into()
            }]
        );
    }

    #[test]
    fn contract_depending_on_core_fails() {
        let m = meta(
            &[
                (
                    "rustev-contract",
                    "crates/rustev-contract",
                    &[("rustev-core", None, Some("crates/rustev-core"))],
                ),
                CORE,
            ],
            &[],
            &[],
        );
        assert_eq!(
            check(&m),
            vec![Violation::WorkspaceEdge {
                krate: "rustev-contract".into(),
                dependency: "rustev-core".into()
            }]
        );
    }

    #[test]
    fn core_depending_on_another_workspace_crate_fails() {
        let m = meta(
            &[
                CONTRACT,
                ("rustev-runtime", "crates/rustev-runtime", &[]),
                (
                    "rustev-core",
                    "crates/rustev-core",
                    &[
                        ("rustev-contract", None, Some("crates/rustev-contract")),
                        ("rustev-runtime", None, Some("crates/rustev-runtime")),
                    ],
                ),
            ],
            &["serde"],
            &[],
        );
        assert_eq!(
            check(&m),
            vec![Violation::WorkspaceEdge {
                krate: "rustev-core".into(),
                dependency: "rustev-runtime".into()
            }]
        );
    }

    const PACKAGE: &str = "packages/rustev-pkg-x";

    #[test]
    fn a_package_on_contract_and_core_with_dev_only_extras_passes() {
        let m = meta(
            &[
                CONTRACT,
                CORE,
                ("rustev-runtime", "crates/rustev-runtime", &[]),
                (
                    "rustev-pkg-x",
                    PACKAGE,
                    &[
                        ("rustev-contract", None, Some("crates/rustev-contract")),
                        ("rustev-core", None, Some("crates/rustev-core")),
                        ("rustev-runtime", Some("dev"), Some("crates/rustev-runtime")),
                        ("tokio", Some("dev"), None),
                    ],
                ),
            ],
            &["serde", "tokio"],
            &[
                ("rustev-core", "rustev-contract", None),
                ("rustev-pkg-x", "rustev-core", None),
                ("rustev-pkg-x", "tokio", Some("dev")),
            ],
        );
        assert_eq!(check(&m), vec![]);
    }

    #[test]
    fn a_package_with_a_normal_dependency_on_the_runtime_fails_naming_the_rule() {
        for kind in [None, Some("build")] {
            let deps: &[Dep<'_>] = &[
                ("rustev-contract", None, Some("crates/rustev-contract")),
                ("rustev-runtime", kind, Some("crates/rustev-runtime")),
            ];
            let m = meta(
                &[
                    CONTRACT,
                    ("rustev-runtime", "crates/rustev-runtime", &[]),
                    ("rustev-pkg-x", PACKAGE, deps),
                ],
                &["serde"],
                &[],
            );
            let v = check(&m);
            assert_eq!(
                v,
                vec![Violation::PackageDependency {
                    krate: "rustev-pkg-x".into(),
                    dependency: "rustev-runtime".into()
                }],
                "{kind:?}"
            );
            assert!(v[0].to_string().contains("007 3.3.1 package rule"));
        }
    }

    #[test]
    fn a_package_reaching_an_executor_through_its_graph_fails() {
        let m = meta(
            &[
                CONTRACT,
                (
                    "rustev-pkg-x",
                    PACKAGE,
                    &[("rustev-contract", None, Some("crates/rustev-contract"))],
                ),
            ],
            &["serde", "helper", "tokio"],
            &[
                ("rustev-pkg-x", "rustev-contract", None),
                ("rustev-contract", "helper", None),
                ("helper", "tokio", None),
            ],
        );
        assert!(check(&m).contains(&Violation::ForbiddenTransitive {
            krate: "rustev-pkg-x".into(),
            path: vec![
                "rustev-pkg-x".into(),
                "rustev-contract".into(),
                "helper".into(),
                "tokio".into()
            ],
        }));
    }

    #[test]
    fn the_node_api_family_is_named_exactly_in_both_rules() {
        // One membership test for 3.14.1 and 3.14.3: the four names, no
        // prefix match.
        for n in NAPI_FAMILY {
            assert!(is_binding(n) && is_napi(n), "{n}");
        }
        assert!(!is_napi("napi-helper") && !is_binding("napi-helper"));
    }

    #[test]
    fn a_binding_family_outside_integrations_fails() {
        // Spec 027, 3.14.1: every Node-API and WebAssembly family.
        for dep in BINDING_FAMILIES {
            let deps: &[Dep<'_>] = &[(dep, None, None)];
            let m = meta(
                &[CONTRACT, ("rustev-x", "crates/rustev-x", deps)],
                &["serde", dep],
                &[],
            );
            assert_eq!(
                check(&m),
                vec![Violation::ForbiddenFamily {
                    krate: "rustev-x".into(),
                    dependency: dep.to_string()
                }],
                "{dep}"
            );
        }
    }

    #[test]
    fn the_binding_crates_under_integrations_pass() {
        let m = meta(
            &[
                CONTRACT,
                CORE,
                (
                    "rustev-node",
                    "integrations/rustev-node",
                    &[
                        ("rustev-core", None, Some("crates/rustev-core")),
                        ("napi", None, None),
                        ("napi-derive", None, None),
                        ("napi-build", Some("build"), None),
                        ("tokio", None, None),
                    ],
                ),
                (
                    "rustev-wasm",
                    "integrations/rustev-wasm",
                    &[
                        ("rustev-core", None, Some("crates/rustev-core")),
                        ("wasm-bindgen", None, None),
                        ("js-sys", None, None),
                    ],
                ),
            ],
            &[
                "serde",
                "napi",
                "napi-derive",
                "napi-build",
                "tokio",
                "wasm-bindgen",
                "js-sys",
            ],
            &[
                ("rustev-core", "rustev-contract", None),
                ("rustev-node", "rustev-core", None),
                ("rustev-node", "napi", None),
                ("rustev-node", "napi-derive", None),
                ("rustev-node", "napi-build", Some("build")),
                ("rustev-node", "tokio", None),
                ("rustev-wasm", "rustev-core", None),
                ("rustev-wasm", "wasm-bindgen", None),
                ("rustev-wasm", "js-sys", None),
            ],
        );
        assert_eq!(check(&m), vec![]);
    }

    #[test]
    fn a_pure_crate_reaching_a_binding_family_fails_and_the_surface_is_checked() {
        // Spec 027, 3.14.2: contract and core refuse the families
        // transitively, and `rustev-surface` joins the checked set.
        let surface: Member<'static> = (
            "rustev-surface",
            "crates/rustev-surface",
            &[("rustev-core", None, Some("crates/rustev-core"))],
        );
        let m = meta(
            &[CONTRACT, CORE, surface],
            &["serde", "helper", "wasm-bindgen"],
            &[
                ("rustev-core", "rustev-contract", None),
                ("rustev-surface", "rustev-core", None),
                ("rustev-contract", "helper", None),
                ("helper", "wasm-bindgen", None),
            ],
        );
        let v = check(&m);
        for krate in ["rustev-contract", "rustev-core", "rustev-surface"] {
            assert!(
                v.iter().any(
                    |x| matches!(x, Violation::ForbiddenTransitive { krate: k, path }
                    if k == krate && path.last().map(String::as_str) == Some("wasm-bindgen"))
                ),
                "{krate}: {v:?}"
            );
        }
        // An executor reached by the surface fails too.
        let m = meta(
            &[CONTRACT, CORE, surface],
            &["serde", "tokio"],
            &[
                ("rustev-core", "rustev-contract", None),
                ("rustev-surface", "rustev-core", None),
                ("rustev-surface", "tokio", None),
            ],
        );
        assert_eq!(
            check(&m),
            vec![Violation::ForbiddenTransitive {
                krate: "rustev-surface".into(),
                path: vec!["rustev-surface".into(), "tokio".into()]
            }]
        );
    }

    #[test]
    fn the_wasm_graph_refuses_each_forbidden_crate() {
        // Spec 027, 3.14.3, through a helper so the path is named.
        for bad in [
            "rustev-runtime",
            "tokio",
            "hyper",
            "napi",
            "napi-sys",
            "getrandom",
            "web-sys",
            "wasm-bindgen-futures",
        ] {
            let m = meta(
                &[
                    CONTRACT,
                    (
                        "rustev-wasm",
                        "integrations/rustev-wasm",
                        &[("helper", None, None)],
                    ),
                ],
                &["serde", "helper", bad],
                &[("rustev-wasm", "helper", None), ("helper", bad, None)],
            );
            let v = check(&m);
            assert!(
                v.contains(&Violation::WasmGraph {
                    path: vec!["rustev-wasm".into(), "helper".into(), bad.into()]
                }),
                "{bad}: {v:?}"
            );
            assert!(v.iter().any(|x| x.to_string().starts_with("027 3.14.3")));
        }
        // A dev-only edge is outside the graph; js-sys and wasm-bindgen are
        // allowed.
        let m = meta(
            &[
                CONTRACT,
                (
                    "rustev-wasm",
                    "integrations/rustev-wasm",
                    &[("wasm-bindgen", None, None), ("js-sys", None, None)],
                ),
            ],
            &["serde", "wasm-bindgen", "js-sys", "getrandom"],
            &[
                ("rustev-wasm", "wasm-bindgen", None),
                ("rustev-wasm", "js-sys", None),
                ("rustev-wasm", "getrandom", Some("dev")),
            ],
        );
        assert_eq!(check(&m), vec![]);
    }

    #[test]
    fn statecraft_and_spec_spine_prefixes_are_families() {
        assert!(is_ecosystem("statecraft-cli"));
        assert!(is_ecosystem("spec-spine-core"));
        assert!(is_ecosystem("aicortex-read"));
        assert!(!is_ecosystem("serde"));
    }

    // ---- spec 031 3.3: the facade ------------------------------------------

    /// The facade over contract, core and the remote binding, with
    /// `remote-http` enabling the binding as an optional dependency; `edit`
    /// adjusts the metadata before the check.
    fn facade_ws(edit: impl FnOnce(&mut Metadata)) -> Vec<Violation> {
        let mut m = meta(
            &[
                CONTRACT,
                CORE,
                (
                    "rustev-remote-http",
                    "integrations/rustev-remote-http",
                    &[("rustev-core", None, Some("crates/rustev-core"))],
                ),
                (
                    FACADE,
                    "integrations/rustev",
                    &[
                        ("rustev-contract", None, Some("crates/rustev-contract")),
                        ("rustev-core", None, Some("crates/rustev-core")),
                        (
                            "rustev-remote-http",
                            None,
                            Some("integrations/rustev-remote-http"),
                        ),
                    ],
                ),
            ],
            &["serde"],
            &[
                ("rustev-contract", "serde", None),
                ("rustev-core", "rustev-contract", None),
                ("rustev-remote-http", "rustev-core", None),
                (FACADE, "rustev-contract", None),
                (FACADE, "rustev-core", None),
            ],
        );
        let f = m.packages.iter_mut().find(|p| p.name == FACADE).unwrap();
        f.dependencies
            .iter_mut()
            .find(|d| d.name == "rustev-remote-http")
            .unwrap()
            .optional = true;
        f.features = BTreeMap::from([
            ("default".to_string(), vec![]),
            (
                "remote-http".to_string(),
                vec!["dep:rustev-remote-http".to_string()],
            ),
        ]);
        edit(&mut m);
        check(&m)
    }

    fn facade(m: &mut Metadata) -> &mut Package {
        m.packages.iter_mut().find(|p| p.name == FACADE).unwrap()
    }

    #[test]
    fn the_facade_with_an_optional_integration_behind_one_feature_passes() {
        assert_eq!(facade_ws(|_| {}), vec![]);
    }

    #[test]
    fn the_facade_depending_on_an_integration_without_optional_fails() {
        let v = facade_ws(|m| {
            facade(m)
                .dependencies
                .iter_mut()
                .for_each(|d| d.optional = false);
        });
        assert_eq!(
            v,
            vec![Violation::FacadeIntegration {
                dependency: "rustev-remote-http".into(),
                detail: "without `optional = true`".into()
            }]
        );
    }

    #[test]
    fn the_facade_enabling_an_integration_by_two_features_or_none_fails() {
        let two = facade_ws(|m| {
            facade(m)
                .features
                .insert("http".into(), vec!["dep:rustev-remote-http".into()]);
        });
        assert_eq!(
            two,
            vec![Violation::FacadeIntegration {
                dependency: "rustev-remote-http".into(),
                detail: "enabled by 2 features, not exactly one".into()
            }]
        );
        let none = facade_ws(|m| {
            facade(m).features.remove("remote-http");
        });
        assert_eq!(
            none,
            vec![Violation::FacadeIntegration {
                dependency: "rustev-remote-http".into(),
                detail: "enabled by 0 features, not exactly one".into()
            }]
        );
    }

    #[test]
    fn a_workspace_crate_depending_on_the_facade_fails_in_every_kind() {
        for kind in [None, Some("dev"), Some("build")] {
            // `rustev-eval`, which no other rule restricts here.
            let v = facade_ws(|m| {
                m.workspace_members.push("rustev-eval 0.1.0".into());
                m.packages.push(Package {
                    id: "rustev-eval 0.1.0".into(),
                    name: "rustev-eval".into(),
                    manifest_path: "/ws/crates/rustev-eval/Cargo.toml".into(),
                    dependencies: vec![Dependency {
                        name: FACADE.into(),
                        kind: kind.map(str::to_string),
                        path: Some("/ws/integrations/rustev".into()),
                        optional: false,
                    }],
                    features: BTreeMap::new(),
                });
            });
            // Exactly one violation, named by 031 3.3.2.
            assert_eq!(
                v,
                vec![Violation::DependsOnFacade {
                    krate: "rustev-eval".into()
                }],
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_non_empty_default_feature_fails() {
        let v = facade_ws(|m| {
            facade(m)
                .features
                .insert("default".into(), vec!["remote-http".into()]);
        });
        assert_eq!(
            v,
            vec![Violation::FacadeDefault {
                enables: vec!["remote-http".into()]
            }]
        );
    }

    #[test]
    fn the_facade_reaching_an_executor_with_no_features_fails() {
        // A non-optional path to an executor is in the default-feature graph.
        let v = facade_ws(|m| {
            m.packages.push(Package {
                id: "tokio 0.1.0".into(),
                name: "tokio".into(),
                manifest_path: "/registry/tokio/Cargo.toml".into(),
                dependencies: vec![],
                features: BTreeMap::new(),
            });
            let r = m.resolve.as_mut().unwrap();
            r.nodes.push(Node {
                id: "tokio 0.1.0".into(),
                deps: vec![],
            });
            let core = r
                .nodes
                .iter_mut()
                .find(|n| n.id == "rustev-core 0.1.0")
                .unwrap();
            core.deps.push(NodeDep {
                pkg: "tokio 0.1.0".into(),
                dep_kinds: vec![DepKind { kind: None }],
            });
        });
        assert!(v.contains(&Violation::ForbiddenTransitive {
            krate: FACADE.into(),
            path: vec![FACADE.into(), "rustev-core".into(), "tokio".into()]
        }));
    }

    #[test]
    fn the_facade_declaring_a_family_directly_fails() {
        let v = facade_ws(|m| {
            facade(m).dependencies.push(Dependency {
                name: "hyper".into(),
                kind: None,
                path: None,
                optional: true,
            });
        });
        assert_eq!(
            v,
            vec![Violation::FacadeFamily {
                dependency: "hyper".into()
            }]
        );
    }

    #[test]
    fn another_integration_depending_on_an_integration_still_fails() {
        // The exception names the facade alone (3.3): a second integration
        // crate depending on the remote binding still breaks 001 3.4.4.
        let v = facade_ws(|m| {
            m.workspace_members.push("rustev-serve 0.1.0".into());
            m.packages.push(Package {
                id: "rustev-serve 0.1.0".into(),
                name: "rustev-serve".into(),
                manifest_path: "/ws/integrations/rustev-serve/Cargo.toml".into(),
                dependencies: vec![Dependency {
                    name: "rustev-remote-http".into(),
                    kind: None,
                    path: Some("/ws/integrations/rustev-remote-http".into()),
                    optional: true,
                }],
                features: BTreeMap::new(),
            });
        });
        assert_eq!(
            v,
            vec![Violation::DependsOnIntegration {
                krate: "rustev-serve".into(),
                dependency: "rustev-remote-http".into()
            }]
        );
    }

    #[test]
    fn a_transitive_violation_stands_only_when_the_crate_own_graph_confirms_it() {
        // Spec 001 3.5 (R-45): a path the workspace-wide feature resolution shows, but
        // the crate's own resolution does not hold, is not a violation.
        let pure = Violation::ForbiddenTransitive {
            krate: "rustev-contract".into(),
            path: vec![
                "rustev-contract".into(),
                "sha2".into(),
                "getrandom".into(),
                "js-sys".into(),
            ],
        };
        let own = tree_names(
            "rustev-contract v0.1.0 (/ws/crates/rustev-contract)\nsha2 v0.10.9\nserde v1.0.0 (*)\n",
        );
        assert_eq!(own, vec!["rustev-contract", "sha2", "serde"]);
        assert_eq!(own_graph_of(&pure), Some(("rustev-contract", "all")));
        assert!(!confirmed(&pure, &own));
        // The negative control: the crate's own graph holds the family.
        assert!(confirmed(&pure, &["rustev-contract", "tokio"]));
        assert!(confirmed(&pure, &["rustev-contract", "js-sys"]));

        let wasm = Violation::WasmGraph {
            path: vec!["rustev-wasm".into(), "getrandom".into()],
        };
        assert_eq!(
            own_graph_of(&wasm),
            Some((WASM_CRATE, "wasm32-unknown-unknown"))
        );
        // The binding itself reaches js-sys; only the 3.14.3 list counts.
        assert!(!confirmed(
            &wasm,
            &["rustev-wasm", "js-sys", "wasm-bindgen"]
        ));
        assert!(confirmed(&wasm, &["rustev-wasm", "getrandom"]));

        // Every other rule is about declared edges and stands as found.
        let direct = Violation::ForbiddenFamily {
            krate: "rustev-core".into(),
            dependency: "rahi-store".into(),
        };
        assert_eq!(own_graph_of(&direct), None);
        assert!(confirmed(&direct, &[]));
    }

    /// The telemetry exporter family of spec 028, 3.12.1, as named there.
    const TELEMETRY: &[&str] = &[
        "opentelemetry",
        "opentelemetry_sdk",
        "opentelemetry-otlp",
        "opentelemetry-proto",
        "opentelemetry-http",
        "opentelemetry-stdout",
        "opentelemetry-semantic-conventions",
        "opentelemetry-appender-tracing",
        "opentelemetry-appender-log",
        "tracing-opentelemetry",
    ];

    #[test]
    fn the_telemetry_family_is_matched_by_prefix_and_exact_name() {
        for n in TELEMETRY {
            assert!(is_telemetry(n), "{n}");
        }
        for n in ["tracing", "tracing-subscriber", "otel", "open-telemetry"] {
            assert!(!is_telemetry(n), "{n}");
        }
    }

    #[test]
    fn a_telemetry_dependency_outside_integrations_fails_in_normal_and_build() {
        // Spec 028, 3.12.2, negative control: each family member, as a
        // normal and as a build dependency, from crates/, backends/,
        // packages/ and tools/.
        for dep in TELEMETRY {
            for kind in [None, Some("build")] {
                for dir in [
                    "crates/rustev-x",
                    "backends/rustev-x",
                    "packages/rustev-x",
                    "tools/rustev-x",
                ] {
                    let deps: &[Dep<'_>] = &[(dep, kind, None)];
                    let m = meta(
                        &[CONTRACT, ("rustev-x", dir, deps)],
                        &["serde", dep],
                        &[("rustev-x", dep, kind)],
                    );
                    let v = check(&m);
                    assert!(
                        v.contains(&Violation::TelemetryOutsideIntegrations {
                            krate: "rustev-x".into(),
                            path: vec!["rustev-x".into(), dep.to_string()],
                        }),
                        "{dep} {kind:?} {dir}: {v:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn an_optional_telemetry_dependency_outside_integrations_fails() {
        // Declared but absent from the resolved graph (a feature left off):
        // the declaration alone is a violation.
        let deps: &[Dep<'_>] = &[("opentelemetry_sdk", None, None)];
        let m = meta(
            &[CONTRACT, ("rustev-runtime", "crates/rustev-runtime", deps)],
            &["serde", "opentelemetry_sdk"],
            &[],
        );
        assert_eq!(
            check(&m),
            vec![Violation::TelemetryOutsideIntegrations {
                krate: "rustev-runtime".into(),
                path: vec!["rustev-runtime".into(), "opentelemetry_sdk".into()],
            }]
        );
    }

    #[test]
    fn a_transitive_telemetry_dependency_outside_integrations_fails_with_its_path() {
        // Spec 028, 3.12.2 and section 5: a crate under crates/ gaining
        // `opentelemetry` even transitively; the violation names the path.
        let m = meta(
            &[
                CONTRACT,
                ("rustev-cli", "crates/rustev-cli", &[("helper", None, None)]),
            ],
            &["serde", "helper", "inner", "opentelemetry"],
            &[
                ("rustev-cli", "helper", None),
                ("helper", "inner", Some("build")),
                ("inner", "opentelemetry", None),
            ],
        );
        assert_eq!(
            check(&m),
            vec![Violation::TelemetryOutsideIntegrations {
                krate: "rustev-cli".into(),
                path: vec![
                    "rustev-cli".into(),
                    "helper".into(),
                    "inner".into(),
                    "opentelemetry".into()
                ],
            }]
        );
    }

    #[test]
    fn a_dev_only_telemetry_dependency_outside_integrations_passes() {
        // The rule is over normal and build dependencies (3.12.2).
        let deps: &[Dep<'_>] = &[("opentelemetry", Some("dev"), None)];
        let m = meta(
            &[CONTRACT, ("rustev-eval", "crates/rustev-eval", deps)],
            &["serde", "opentelemetry"],
            &[("rustev-eval", "opentelemetry", Some("dev"))],
        );
        assert_eq!(check(&m), vec![]);
    }

    #[test]
    fn the_telemetry_sink_under_integrations_passes() {
        // `rustev-otel` as spec 028 section 2 lays it out, with its OTLP
        // transports (already HTTP families, legal only here).
        let m = meta(
            &[
                CONTRACT,
                CORE,
                (
                    "rustev-otel",
                    "integrations/rustev-otel",
                    &[
                        ("rustev-contract", None, Some("crates/rustev-contract")),
                        ("rustev-core", None, Some("crates/rustev-core")),
                        ("tokio", None, None),
                        ("opentelemetry", None, None),
                        ("opentelemetry_sdk", None, None),
                        ("opentelemetry-otlp", None, None),
                        ("tonic", None, None),
                        ("hyper", None, None),
                    ],
                ),
            ],
            &[
                "serde",
                "tokio",
                "opentelemetry",
                "opentelemetry_sdk",
                "opentelemetry-otlp",
                "opentelemetry-proto",
                "tonic",
                "hyper",
            ],
            &[
                ("rustev-core", "rustev-contract", None),
                ("rustev-otel", "rustev-core", None),
                ("rustev-otel", "opentelemetry_sdk", None),
                ("rustev-otel", "opentelemetry-otlp", None),
                ("opentelemetry-otlp", "opentelemetry-proto", None),
                ("opentelemetry-otlp", "tonic", None),
            ],
        );
        assert_eq!(check(&m), vec![]);
    }

    #[test]
    fn a_pure_crate_reaching_telemetry_fails_under_the_new_rule() {
        // Rule 3.4.1 is unchanged (028 3.12.2); the contract and the core
        // reaching the family are telemetry violations naming the path.
        let m = meta(
            &[CONTRACT, CORE],
            &["serde", "opentelemetry"],
            &[
                ("rustev-core", "rustev-contract", None),
                ("rustev-contract", "serde", None),
                ("serde", "opentelemetry", None),
            ],
        );
        let v = check(&m);
        assert!(v.contains(&Violation::TelemetryOutsideIntegrations {
            krate: "rustev-contract".into(),
            path: vec![
                "rustev-contract".into(),
                "serde".into(),
                "opentelemetry".into()
            ],
        }));
        assert!(v.contains(&Violation::TelemetryOutsideIntegrations {
            krate: "rustev-core".into(),
            path: vec![
                "rustev-core".into(),
                "rustev-contract".into(),
                "serde".into(),
                "opentelemetry".into()
            ],
        }));
    }
}
