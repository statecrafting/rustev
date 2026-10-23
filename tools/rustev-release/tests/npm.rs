//! Spec 031 acceptance: "The release-set file fixes members, layers,
//! absences with reasons, and the feature map". The npm members (3.1, the npm
//! layer) and the absent artifacts.
//!
//! The first tests judge this repository; every other test breaks one rule
//! and expects exactly that violation.

use std::collections::BTreeMap;
use std::path::Path;

use rustev_release::ReleaseSet;
use rustev_release::npm::{NpmViolation, check_npm, check_npm_set};
use serde_json::Value;

const SET: &str = include_str!("../release-set.json");

fn set() -> ReleaseSet {
    ReleaseSet::parse(SET.as_bytes()).unwrap()
}

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn read(p: &str) -> Value {
    serde_json::from_slice(&std::fs::read(root().join(p)).unwrap()).unwrap()
}

/// Judge `s` with the repository's manifests, `edit` applied to the one at
/// `path` first.
fn judge(s: &ReleaseSet, path: &str, edit: impl Fn(&mut Value)) -> Vec<NpmViolation> {
    let mut docs = BTreeMap::new();
    for p in &s.npm {
        let file = format!("{}/package.json", p.directory);
        let mut doc = read(&file);
        if file == path {
            edit(&mut doc);
        }
        docs.insert(file, serde_json::to_vec(&doc).unwrap());
    }
    check_npm(s, |p| {
        docs.get(p).cloned().ok_or_else(|| format!("{p}: absent"))
    })
}

fn only(v: Vec<NpmViolation>) -> NpmViolation {
    assert_eq!(v.len(), 1, "{v:#?}");
    v.into_iter().next().unwrap()
}

fn field_of(v: Vec<NpmViolation>) -> (String, String) {
    match only(v) {
        NpmViolation::Field { name, field, .. } => (name, field),
        other => panic!("{other:?}"),
    }
}

const MAIN: &str = "integrations/rustev-node/package.json";
const LINUX: &str = "integrations/rustev-node/npm/linux-x64-gnu/package.json";
const WASM: &str = "integrations/rustev-wasm/package.json";

#[test]
fn the_repository_passes() {
    let v = check_npm(&set(), |p| {
        std::fs::read(root().join(p)).map_err(|e| e.to_string())
    });
    assert!(v.is_empty(), "{v:#?}");
}

#[test]
fn the_release_set_fixes_the_npm_members_and_the_absences() {
    let s = set();
    let npm: Vec<&str> = s.npm.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        npm,
        [
            "@statecrafting/rustev-darwin-arm64",
            "@statecrafting/rustev-darwin-x64",
            "@statecrafting/rustev-linux-arm64-gnu",
            "@statecrafting/rustev-linux-arm64-musl",
            "@statecrafting/rustev-linux-x64-gnu",
            "@statecrafting/rustev-linux-x64-musl",
            "@statecrafting/rustev-win32-x64-msvc",
            "@statecrafting/rustev",
            "@statecrafting/rustev-wasm",
        ]
    );
    // Every platform package on disk is a member, and no other.
    let mut on_disk: Vec<String> = std::fs::read_dir(root().join("integrations/rustev-node/npm"))
        .unwrap()
        .map(|e| {
            format!(
                "@statecrafting/rustev-{}",
                e.unwrap().file_name().to_string_lossy()
            )
        })
        .collect();
    on_disk.sort();
    assert_eq!(on_disk, npm[..7]);
    let absent: Vec<(&str, &str)> = s
        .absent
        .iter()
        .map(|a| (a.kind.as_str(), a.name.as_str()))
        .collect();
    assert_eq!(
        absent,
        [
            ("image", "ghcr.io/statecrafting/rustev-serve"),
            ("crate", "rustev-bedrock"),
            ("crate", "rustev-backend-embed-linear"),
        ]
    );
    assert!(check_npm_set(&s).is_empty());
}

#[test]
fn a_version_outside_the_family_fails() {
    let v = judge(&set(), LINUX, |d| d["version"] = "0.1.1".into());
    assert_eq!(
        field_of(v),
        (
            "@statecrafting/rustev-linux-x64-gnu".into(),
            "version".into()
        )
    );
}

#[test]
fn a_wrong_name_license_or_repository_fails() {
    let v = judge(&set(), WASM, |d| d["name"] = "rustev-wasm".into());
    assert_eq!(field_of(v).1, "name");
    let v = judge(&set(), WASM, |d| d["license"] = "MIT".into());
    assert_eq!(field_of(v).1, "license");
    let v = judge(&set(), WASM, |d| {
        d["repository"]["directory"] = "integrations/rustev-node".into()
    });
    assert_eq!(field_of(v).1, "repository");
    let v = judge(&set(), WASM, |d| d["description"] = "".into());
    assert_eq!(field_of(v).1, "description");
    let v = judge(&set(), WASM, |d| d["files"] = Value::Array(vec![]));
    assert_eq!(field_of(v).1, "files");
}

#[test]
fn a_member_without_the_private_guard_fails() {
    let v = judge(&set(), MAIN, |d| {
        d.as_object_mut().unwrap().remove("private");
    });
    assert_eq!(
        field_of(v),
        ("@statecrafting/rustev".into(), "private".into())
    );
}

#[test]
fn a_platform_package_without_os_or_cpu_fails() {
    let v = judge(&set(), LINUX, |d| {
        d.as_object_mut().unwrap().remove("cpu");
    });
    assert_eq!(field_of(v).1, "cpu");
}

#[test]
fn the_main_package_names_exactly_the_platform_packages_at_the_family_version() {
    let v = judge(&set(), MAIN, |d| {
        d["optionalDependencies"]
            .as_object_mut()
            .unwrap()
            .remove("@statecrafting/rustev-darwin-x64");
    });
    assert_eq!(field_of(v).1, "optionalDependencies");
    let v = judge(&set(), MAIN, |d| {
        d["optionalDependencies"]["@statecrafting/rustev-darwin-x64"] = "^0.1.0".into()
    });
    assert_eq!(field_of(v).1, "optionalDependencies");
    // A platform the set does not ship is refused too.
    let mut s = set();
    s.npm
        .retain(|p| p.name != "@statecrafting/rustev-darwin-x64");
    let v = judge(&s, "", |_| {});
    assert_eq!(field_of(v).1, "optionalDependencies");
}

#[test]
fn a_runtime_dependency_fails() {
    let v = judge(&set(), WASM, |d| {
        d["dependencies"] = serde_json::json!({ "left-pad": "1.3.0" })
    });
    assert_eq!(field_of(v).1, "dependencies");
    // npm's bundled list is an array (or `true`); an empty one declares
    // nothing, a named one is refused under either spelling.
    for key in ["bundleDependencies", "bundledDependencies"] {
        let v = judge(&set(), WASM, |d| d[key] = serde_json::json!([]));
        assert!(v.is_empty(), "{v:#?}");
        let v = judge(&set(), WASM, |d| d[key] = serde_json::json!(["left-pad"]));
        assert_eq!(field_of(v).1, key);
        let v = judge(&set(), WASM, |d| d[key] = serde_json::json!(true));
        assert_eq!(field_of(v).1, key);
    }
    let v = judge(&set(), WASM, |d| d["dependencies"] = serde_json::json!({}));
    assert!(v.is_empty(), "{v:#?}");
}

#[test]
fn an_unreadable_manifest_fails() {
    let v = check_npm(&set(), |p| {
        if p == WASM {
            Err("gone".into())
        } else {
            std::fs::read(root().join(p)).map_err(|e| e.to_string())
        }
    });
    assert!(
        matches!(only(v), NpmViolation::Unreadable { ref name, .. } if name == "@statecrafting/rustev-wasm")
    );
}

fn set_detail_of(v: Vec<NpmViolation>) -> String {
    let sets: Vec<String> = v
        .iter()
        .filter_map(|x| match x {
            NpmViolation::Set { detail } => Some(detail.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(sets.len(), 1, "{v:#?}");
    sets[0].clone()
}

fn set_detail(s: &ReleaseSet) -> String {
    match only(check_npm_set(s)) {
        NpmViolation::Set { detail } => detail,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_npm_layer_is_ordered() {
    // A platform package after the main package that names it.
    let mut s = set();
    let p = s.npm.remove(0);
    s.npm.insert(7, p);
    assert!(
        check_npm_set(&s)
            .iter()
            .any(|v| v.to_string().contains("uploads after"))
    );
    // The WebAssembly package not last.
    let mut s = set();
    let w = s.npm.pop().unwrap();
    s.npm.insert(7, w);
    assert!(set_detail(&s).contains("does not upload last"));
    // Platform packages out of name order.
    let mut s = set();
    s.npm.swap(0, 1);
    assert!(set_detail(&s).contains("ordered by name"));
}

#[test]
fn the_npm_list_shape_is_checked() {
    let mut s = set();
    let dup = s.npm[8].clone();
    s.npm.push(dup);
    assert!(
        check_npm_set(&s)
            .iter()
            .any(|v| v.to_string().contains("listed twice"))
    );
    let mut s = set();
    s.npm[8].directory = "integrations/rustev-node".into();
    assert!(set_detail(&s).contains("is built from"));
    // A directory or a platform name that leaves the family's directories
    // is refused, and nothing is read from it.
    let mut s = set();
    s.npm[8].directory = "integrations/rustev-wasm/../../etc".into();
    let v = check_npm(&s, |p| {
        assert!(!p.contains(".."), "read {p}");
        std::fs::read(root().join(p)).map_err(|e| e.to_string())
    });
    assert!(set_detail_of(v).contains("is built from"));
    let mut s = set();
    s.npm[0].name = "@statecrafting/rustev-../../etc".into();
    s.npm[0].directory = "integrations/rustev-node/npm/../../etc".into();
    let v = check_npm(&s, |p| {
        assert!(!p.contains(".."), "read {p}");
        std::fs::read(root().join(p)).map_err(|e| e.to_string())
    });
    assert!(
        v.iter()
            .any(|x| x.to_string().contains("not a family package")),
        "{v:#?}"
    );
    let mut s = set();
    s.npm[8].name = "@other/rustev-wasm".into();
    assert!(
        check_npm_set(&s)
            .iter()
            .any(|v| v.to_string().contains("scope"))
    );
}

#[test]
fn absences_carry_a_known_kind_and_a_reason_and_are_not_members() {
    let mut s = set();
    s.absent[0].reason = " ".into();
    assert!(set_detail(&s).contains("has no reason"));
    let mut s = set();
    s.absent[0].kind = "service".into();
    assert!(set_detail(&s).contains("unknown kind"));
    let mut s = set();
    s.absent[2].name = "rustev-cli".into();
    assert!(set_detail(&s).contains("both a member and absent"));
    let mut s = set();
    s.absent[2].kind = "npm".into();
    s.absent[2].name = "@statecrafting/rustev-wasm".into();
    assert!(set_detail(&s).contains("both a member and absent"));
    let mut s = set();
    let dup = s
        .absent
        .first()
        .expect("release has absent members")
        .clone();
    s.absent.push(dup);
    assert!(set_detail(&s).contains("listed twice"));
}
