//! Spec 021 acceptance: "A machine-checked release-set file ... fixes the
//! eight names, version, layers, exclusions and exact internal dependency
//! policy" and "Manifest tests prove complete metadata, no public features,
//! intended registry restriction, package versus binary naming, and absence
//! of excluded closure."
//!
//! The first test judges this workspace; every other test breaks one rule in
//! the workspace's own metadata and expects exactly that violation.

use std::process::Command;
use std::sync::OnceLock;

use rustev_release::{Metadata, ReleaseSet, Violation, check_manifests, check_set};

const SET: &str = include_str!("../release-set.json");

fn set() -> ReleaseSet {
    ReleaseSet::parse(SET.as_bytes()).unwrap()
}

fn metadata() -> Metadata {
    static META: OnceLock<Vec<u8>> = OnceLock::new();
    let bytes = META.get_or_init(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let out = Command::new(cargo)
            .args(["metadata", "--format-version", "1", "--locked"])
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    });
    serde_json::from_slice(bytes).unwrap()
}

fn package<'a>(m: &'a mut Metadata, name: &str) -> &'a mut rustev_release::Package {
    m.packages.iter_mut().find(|p| p.name == name).unwrap()
}

fn only(v: Vec<Violation>) -> Violation {
    assert_eq!(v.len(), 1, "{v:#?}");
    v.into_iter().next().unwrap()
}

#[test]
fn the_workspace_passes_the_release_set() {
    let v = check_manifests(&set(), &metadata());
    assert!(
        v.is_empty(),
        "{}",
        v.iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_release_set_fixes_the_first_family() {
    let s = set();
    // Spec 021 3.2: the set names the family version its members carry.
    let family = package(&mut metadata(), "rustev-contract").version.clone();
    assert_eq!(s.version, family);
    assert_eq!(s.tag, format!("v{family}"));
    let layers: Vec<Vec<&str>> = s
        .layers
        .iter()
        .map(|l| l.iter().map(String::as_str).collect())
        .collect();
    assert_eq!(
        layers,
        [
            vec!["rustev-contract"],
            vec!["rustev-core"],
            vec![
                "rustev-backend-rules",
                "rustev-eval",
                // Spec 031 3.1: lodging joins the family.
                "rustev-pkg-lodging",
                "rustev-pkg-support-routing",
                "rustev-remote-http",
                "rustev-runtime"
            ],
            // Spec 045: the command surface the CLI depends on (spec 027), and
            // the live run engine it depends on (spec 043, under 045 3.1.2).
            vec!["rustev-live", "rustev-otel", "rustev-surface"],
            vec!["rustev-cli"],
            // Spec 031 3.1: the facade, always the last crate layer.
            vec!["rustev"],
        ]
    );
    // Every workspace package outside the members is excluded, by name.
    let mut excluded: Vec<&str> = s.excluded.iter().map(|e| e.name.as_str()).collect();
    excluded.sort();
    let m = metadata();
    let mut outside: Vec<&str> = m
        .packages
        .iter()
        .filter(|p| m.workspace_members.contains(&p.id))
        .map(|p| p.name.as_str())
        .filter(|n| !s.members().any(|x| x == *n))
        .collect();
    outside.sort();
    assert_eq!(excluded, outside);
    for name in ["rustev-boundaries", "rustev-release"] {
        assert!(excluded.contains(&name), "{name} must be excluded (021 2)");
    }
    // The provider adapter (section 2's first exclusion) is identified by its
    // stated reason: spec 012 I-5 keeps the provider's name out of Rust
    // sources under tools/, so this test does not spell it.
    assert!(
        s.excluded.iter().any(|e| e.name.starts_with("rustev-")
            && e.reason
                .starts_with("source-links sibling integration files")),
        "the source-linking provider adapter must be excluded (021 2)"
    );
    assert!(check_set(&s).is_empty());
}

#[test]
fn a_path_only_internal_dependency_fails() {
    let mut m = metadata();
    let d = package(&mut m, "rustev-core")
        .dependencies
        .iter_mut()
        .find(|d| d.name == "rustev-contract")
        .unwrap();
    d.req = "*".into();
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::InternalDependency { ref name, ref dependency, .. }
            if name == "rustev-core" && dependency == "rustev-contract"
    ));
}

#[test]
fn an_internal_dependency_at_another_version_fails() {
    // Section 4: "A package resolves an internal crate at 0.1.1 during a 0.1.0 release".
    let mut m = metadata();
    let d = package(&mut m, "rustev-cli")
        .dependencies
        .iter_mut()
        .find(|d| d.name == "rustev-runtime")
        .unwrap();
    d.req = "^0.1.0".into();
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::InternalDependency { .. }
    ));
}

#[test]
fn a_dev_dependency_on_an_excluded_crate_fails() {
    let mut m = metadata();
    let mut excluded = package(&mut m, "rustev-cli").dependencies[0].clone();
    excluded.name = "rustev-boundaries".into();
    excluded.kind = Some("dev".into());
    package(&mut m, "rustev-eval").dependencies.push(excluded);
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::InternalDependency { ref dependency, ref kind, .. }
            if dependency == "rustev-boundaries" && kind == "dev"
    ));
}

#[test]
fn a_missing_member_reports_only_itself() {
    let mut m = metadata();
    let id = m
        .packages
        .iter()
        .find(|p| p.name == "rustev-contract")
        .unwrap()
        .id
        .clone();
    m.workspace_members.retain(|w| *w != id);
    m.packages.retain(|p| p.id != id);
    let v = check_manifests(&set(), &m);
    assert!(
        v.iter()
            .all(|v| matches!(v, Violation::MissingMember { .. })),
        "{v:#?}"
    );
}

#[test]
fn a_member_that_may_publish_anywhere_fails() {
    let mut m = metadata();
    package(&mut m, "rustev-eval").publish = None;
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::Publish { .. }
    ));
}

#[test]
fn a_publishable_exclusion_fails() {
    let mut m = metadata();
    package(&mut m, "rustev-boundaries").publish = Some(vec!["crates-io".into()]);
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::Publish { .. }
    ));
}

#[test]
fn missing_metadata_fails_per_field() {
    for field in [
        "description",
        "homepage",
        "documentation",
        "readme",
        "repository",
    ] {
        let mut m = metadata();
        let p = package(&mut m, "rustev-runtime");
        match field {
            "description" => p.description = None,
            "homepage" => p.homepage = None,
            "documentation" => p.documentation = Some("https://example.invalid".into()),
            "readme" => p.readme = None,
            _ => p.repository = None,
        }
        assert!(
            matches!(only(check_manifests(&set(), &m)), Violation::Metadata { field: ref f, .. } if f == field),
            "{field}"
        );
    }
    let mut m = metadata();
    package(&mut m, "rustev-core").keywords = vec!["Not A Keyword".into()];
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::Metadata { .. }
    ));
    let mut m = metadata();
    package(&mut m, "rustev-core").categories.clear();
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::Metadata { .. }
    ));
    let mut m = metadata();
    package(&mut m, "rustev-core").license = Some("MIT".into());
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::Metadata { .. }
    ));
}

#[test]
fn a_cargo_feature_fails() {
    let mut m = metadata();
    package(&mut m, "rustev-contract")
        .features
        .insert("default".into(), vec![]);
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::Feature { .. }
    ));
}

#[test]
fn each_facade_feature_names_one_released_dependency() {
    // Spec 031 3.2 and I-3: a feature naming a crate outside the release
    // (here `bedrock`, whose crate does not exist yet) fails.
    let mut m = metadata();
    package(&mut m, "rustev")
        .features
        .insert("bedrock".into(), vec!["dep:rustev-bedrock".into()]);
    // Exactly one violation for that feature.
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::FacadeFeature { ref feature, ref detail }
            if feature == "bedrock" && detail.contains("not a member of this release")
    ));
    // A non-empty default fails.
    let mut m = metadata();
    package(&mut m, "rustev")
        .features
        .insert("default".into(), vec!["runtime".into()]);
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::FacadeFeature { ref feature, .. } if feature == "default"
    ));
    // A feature enabling two dependencies fails.
    let mut m = metadata();
    package(&mut m, "rustev").features.insert(
        "runtime".into(),
        vec!["dep:rustev-runtime".into(), "dep:rustev-eval".into()],
    );
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::FacadeFeature { ref feature, .. } if feature == "runtime"
    ));
}

#[test]
fn the_facade_is_alone_in_the_last_layer_and_uploads_last() {
    // Placed one layer early (beside the CLI), it fails the layer rule and
    // the set's shape (`check_manifests` begins with `check_set`'s verdict).
    let mut s = set();
    let last = s.layers.pop().unwrap();
    s.layers.last_mut().unwrap().extend(last);
    s.layers.last_mut().unwrap().sort();
    let v = check_manifests(&s, &metadata());
    assert!(
        v.iter()
            .any(|x| matches!(x, Violation::ReleaseSet { detail }
            if detail.contains("not alone in the last layer"))),
        "{v:#?}"
    );
    let mut s = set();
    s.upload_order.retain(|n| n != "rustev");
    s.upload_order.insert(0, "rustev".into());
    let v = check_set(&s);
    assert!(
        v.iter()
            .any(|x| matches!(x, Violation::ReleaseSet { detail }
            if detail.contains("does not upload last"))),
        "{v:#?}"
    );
}

#[test]
fn a_git_or_alternate_registry_source_fails() {
    let mut m = metadata();
    let serde = m.packages.iter_mut().find(|p| p.name == "serde").unwrap();
    serde.source = Some("git+https://example.invalid/serde".into());
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::Source { .. }
    ));

    let mut m = metadata();
    let d = package(&mut m, "rustev-core")
        .dependencies
        .iter_mut()
        .find(|d| d.name == "serde")
        .unwrap();
    d.registry = Some("https://example.invalid/index".into());
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::Source { .. }
    ));
}

#[test]
fn a_git_source_only_an_excluded_package_reaches_is_not_judged() {
    // 3.4 (R-49): the source rule covers the members' closure. A Git
    // package that only an excluded workspace package depends on is outside
    // it; the same package in the closure fails, and without a resolve
    // graph every package is judged.
    let base = metadata();
    let excluded = set().excluded[0].name.clone();
    let excluded_id = base
        .packages
        .iter()
        .find(|p| p.name == excluded && base.workspace_members.contains(&p.id))
        .unwrap()
        .id
        .clone();
    let with_git = |attach_to: &str| {
        let mut m = base.clone();
        let mut git = m
            .packages
            .iter()
            .find(|p| p.name == "serde")
            .unwrap()
            .clone();
        git.name = "git-only".into();
        git.id = "git+https://example.invalid/git-only#git-only@0.1.0".into();
        git.source = Some("git+https://example.invalid/git-only?rev=0".into());
        let id = git.id.clone();
        m.packages.push(git);
        let resolve = m.resolve.as_mut().unwrap();
        resolve.nodes.push(rustev_release::audit::Node {
            id: id.clone(),
            deps: vec![],
        });
        let owner = resolve
            .nodes
            .iter_mut()
            .find(|n| n.id == attach_to)
            .unwrap();
        owner.deps.push(rustev_release::audit::NodeDep {
            pkg: id,
            dep_kinds: vec![],
        });
        m
    };

    let m = with_git(&excluded_id);
    assert!(check_manifests(&set(), &m).is_empty(), "{excluded}");

    let core = base
        .packages
        .iter()
        .find(|p| p.name == "rustev-core")
        .unwrap()
        .id
        .clone();
    let m = with_git(&core);
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::Source { .. }
    ));

    // Without a resolve graph every resolved package is judged: the added
    // package fails, and so does any Git package an excluded integration
    // really has (`rustev-aicortex`), so only the kind is asserted.
    let mut m = with_git(&excluded_id);
    m.resolve = None;
    let v = check_manifests(&set(), &m);
    assert!(
        v.iter()
            .any(|v| matches!(v, Violation::Source { name, .. } if name == "git-only")),
        "{v:#?}"
    );
    assert!(
        v.iter().all(|v| matches!(v, Violation::Source { .. })),
        "{v:#?}"
    );
}

#[test]
fn a_patched_path_package_fails() {
    let mut m = metadata();
    let serde = m.packages.iter_mut().find(|p| p.name == "serde").unwrap();
    serde.source = None;
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::Source { .. }
    ));
}

#[test]
fn package_and_binary_names_are_distinct() {
    let m = metadata();
    let cli = m.packages.iter().find(|p| p.name == "rustev-cli").unwrap();
    let bins: Vec<&str> = cli
        .targets
        .iter()
        .filter(|t| t.kind.iter().any(|k| k == "bin"))
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(bins, ["rustev"]);

    let mut m = metadata();
    package(&mut m, "rustev-cli")
        .targets
        .iter_mut()
        .find(|t| t.kind.iter().any(|k| k == "bin"))
        .unwrap()
        .name = "rustev-cli".into();
    assert!(matches!(
        only(check_manifests(&set(), &m)),
        Violation::Binary { .. }
    ));
}

#[test]
fn layers_and_upload_order_follow_the_graph() {
    let mut s = set();
    s.layers[2].retain(|n| n != "rustev-runtime");
    s.layers[3].push("rustev-runtime".into());
    s.layers[3].sort();
    let v = check_manifests(&s, &metadata());
    assert!(
        v.contains(&Violation::Layer {
            name: "rustev-runtime".into(),
            declared: 4,
            expected: 3
        }),
        "{v:#?}"
    );

    // rustev-pkg-support-routing dev-depends on rustev-cli, so it uploads after it.
    let mut s = set();
    s.upload_order.retain(|n| n != "rustev-pkg-support-routing");
    s.upload_order
        .insert(6, "rustev-pkg-support-routing".into());
    assert!(matches!(
        only(check_manifests(&s, &metadata())),
        Violation::Order { ref name, ref dependency }
            if name == "rustev-pkg-support-routing" && dependency == "rustev-cli"
    ));
}

#[test]
fn the_release_set_shape_is_checked() {
    let mut s = set();
    s.tag = "0.1.0".into();
    assert_eq!(check_set(&s).len(), 1);
    let mut s = set();
    s.version = "0.1".into();
    assert_eq!(check_set(&s).len(), 2, "version and tag");
    let mut s = set();
    s.upload_order.retain(|n| n != "rustev-core");
    assert_eq!(check_set(&s).len(), 1);
    let mut s = set();
    s.excluded[0].name = "rustev-core".into();
    assert!(!check_set(&s).is_empty());
    let mut s = set();
    s.excluded.retain(|e| e.name != "rustev-boundaries");
    assert!(matches!(
        only(check_manifests(&s, &metadata())),
        Violation::Unlisted { ref name } if name == "rustev-boundaries"
    ));
}

// Spec 031 3.2.3 and 028 3.2.3: the allowance is exact, with no default.
#[test]
fn a_missing_allowed_member_feature_fails() {
    let mut m = metadata();
    package(&mut m, "rustev-otel").features.remove("otlp");
    assert!(
        matches!(only(check_manifests(&set(), &m)), Violation::Feature { feature, .. } if feature == "otlp")
    );
}

#[test]
fn an_extra_member_feature_fails() {
    let mut m = metadata();
    package(&mut m, "rustev-otel")
        .features
        .insert("extra".into(), vec![]);
    assert!(
        matches!(only(check_manifests(&set(), &m)), Violation::Feature { feature, .. } if feature == "extra")
    );
}

#[test]
fn a_member_default_enabling_a_feature_fails() {
    let mut m = metadata();
    package(&mut m, "rustev-otel")
        .features
        .insert("default".into(), vec!["otlp".into()]);
    assert!(
        matches!(only(check_manifests(&set(), &m)), Violation::Feature { feature, .. } if feature == "default")
    );
}

#[test]
fn an_absent_member_default_passes() {
    let mut m = metadata();
    package(&mut m, "rustev-otel").features.remove("default");
    assert!(check_manifests(&set(), &m).is_empty());
}

#[test]
fn an_allowance_for_a_non_member_fails() {
    let mut s = set();
    s.member_features[0].package = "absent-member".into();
    assert!(matches!(only(check_set(&s)), Violation::ReleaseSet { .. }));
}

#[test]
fn invalid_allowances_fail() {
    for case in [
        "duplicate",
        "facade",
        "no-spec",
        "duplicate-feature",
        "default",
    ] {
        let mut s = set();
        match case {
            "duplicate" => s.member_features.push(s.member_features[0].clone()),
            "facade" => s.member_features[0].package = "rustev".into(),
            "no-spec" => s.member_features[0].spec.clear(),
            "duplicate-feature" => s.member_features[0].features.push("otlp".into()),
            "default" => s.member_features[0].features.push("default".into()),
            _ => unreachable!(),
        }
        assert!(
            matches!(only(check_set(&s)), Violation::ReleaseSet { .. }),
            "{case}"
        );
    }
}

#[test]
fn telemetry_upload_waits_for_its_development_dependency() {
    let mut s = set();
    s.upload_order.retain(|n| n != "rustev-otel");
    let rules = s
        .upload_order
        .iter()
        .position(|n| n == "rustev-backend-rules")
        .unwrap();
    s.upload_order.insert(rules, "rustev-otel".into());
    assert!(
        matches!(only(check_manifests(&s, &metadata())), Violation::Order { name, dependency } if name == "rustev-otel" && dependency == "rustev-backend-rules")
    );
}
