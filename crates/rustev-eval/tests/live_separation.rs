//! Spec 043, 3.1.2: normal dependencies cannot enter the live execution path.

fn forbidden_dependencies(manifest: &str) -> Vec<String> {
    let forbidden = ["rustev-runtime", "rustev-live", "tokio"];
    let mut normal = false;
    let mut found = Vec::new();
    for line in manifest.lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.starts_with('[') {
            let table = line.trim_matches(['[', ']']).trim();
            // Also recognize dependency subtables and target-specific tables.
            let parts: Vec<_> = table
                .split('.')
                .map(|s| s.trim().trim_matches(['\'', '"']))
                .collect();
            let dependency = parts.iter().position(|p| *p == "dependencies");
            normal = dependency.is_some_and(|i| i == 0 || parts[0] == "target");
            if normal
                && let Some(name) = dependency.and_then(|i| parts.get(i + 1))
                && forbidden.contains(name)
            {
                found.push((*name).into());
            }
        } else if normal && let Some((key, value)) = line.split_once('=') {
            let name = key
                .trim()
                .split('.')
                .next()
                .unwrap()
                .trim_matches(['\'', '"']);
            if forbidden.contains(&name) {
                found.push(name.into());
            }
            // A renamed dependency still names the forbidden package.
            for item in value.trim().trim_matches(['{', '}']).split(',') {
                let package = if name == "package" {
                    Some(value)
                } else {
                    item.split_once('=')
                        .filter(|(k, _)| k.trim() == "package")
                        .map(|(_, v)| v)
                };
                if let Some(package) = package {
                    let package = package.trim().trim_matches(['\'', '"']);
                    if forbidden.contains(&package) {
                        found.push(package.into());
                    }
                }
            }
        }
    }
    found
}

#[test]
fn normal_dependencies_do_not_reach_live_execution() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest = std::fs::read_to_string(path).unwrap();
    assert_eq!(forbidden_dependencies(&manifest), Vec::<String>::new());
}

#[test]
fn reader_covers_target_tables_subtables_and_renames_but_ignores_dev_dependencies() {
    let manifest = r#"
[dependencies]
tokio.workspace = true
[dev-dependencies]
rustev-runtime.workspace = true
[target.'cfg(unix)'.dependencies]
rustev-live = { workspace = true }
renamed = { package = "rustev-runtime", version = "1" }
[target.'cfg(windows)'.dependencies.tokio]
version = "1"
[target.'cfg(unix)'.dev-dependencies]
tokio.workspace = true
[dependencies.renamed]
package = "rustev-live"
"#;
    assert_eq!(
        forbidden_dependencies(manifest),
        [
            "tokio",
            "rustev-live",
            "rustev-runtime",
            "tokio",
            "rustev-live"
        ]
    );
}
