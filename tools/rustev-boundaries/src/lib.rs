//! The workspace dependency rules of spec 001, section 3.4, checked over the
//! output of `cargo metadata --format-version 1`.
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

/// The crates whose whole normal and build dependency graph is checked.
pub const PURE_CRATES: &[&str] = &["rustev-contract", "rustev-core"];

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
}

#[derive(Debug, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub kind: Option<String>,
    pub path: Option<String>,
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
    /// 3.4.5: `rustev-contract` depends on a workspace crate, or `rustev-core`
    /// on one other than `rustev-contract`.
    WorkspaceEdge { krate: String, dependency: String },
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
            Violation::WorkspaceEdge { krate, dependency } => write!(
                f,
                "001 3.4.5: `{krate}` may not depend on the workspace crate `{dependency}`"
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

    for pkg in &members {
        let rel = relative_dir(&meta.workspace_root, &pkg.manifest_path);
        let in_integrations = under_integrations(&rel);
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
            if integration_names.contains(dep.name.as_str()) || dep_under_integrations {
                out.insert(Violation::DependsOnIntegration {
                    krate: pkg.name.clone(),
                    dependency: dep.name.clone(),
                });
            }
            if !in_integrations && (is_ecosystem(&dep.name) || is_http(&dep.name)) {
                out.insert(Violation::ForbiddenFamily {
                    krate: pkg.name.clone(),
                    dependency: dep.name.clone(),
                });
            }
            let is_dev = dep.kind.as_deref() == Some("dev");
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
        for pkg in members
            .iter()
            .filter(|p| PURE_CRATES.contains(&p.name.as_str()))
        {
            if let Some(path) = forbidden_path(&graph, &by_id, &pkg.id) {
                out.insert(Violation::ForbiddenTransitive {
                    krate: pkg.name.clone(),
                    path,
                });
            }
        }
    }
    out.into_iter().collect()
}

/// Breadth-first search for the shortest normal or build path from `start`
/// to a package in a forbidden family; the path lists package names.
fn forbidden_path(
    graph: &BTreeMap<&str, Vec<&str>>,
    by_id: &BTreeMap<&str, &Package>,
    start: &str,
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
            if is_ecosystem(&name) || is_http(&name) || is_executor(&name) {
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

    #[test]
    fn statecraft_and_spec_spine_prefixes_are_families() {
        assert!(is_ecosystem("statecraft-cli"));
        assert!(is_ecosystem("spec-spine-core"));
        assert!(is_ecosystem("aicortex-read"));
        assert!(!is_ecosystem("serde"));
    }
}
