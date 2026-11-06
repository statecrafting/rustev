//! Offline (spec 011, 3.6.1, 3.18.2(c), Acceptance "Offline"): the crate's
//! own source names no network API, and its manifest declares no network
//! client, model hub or download feature. The resolved graph is checked by
//! `make boundaries`; this test checks what the crate itself can call.
use std::fs;
use std::path::{Path, PathBuf};

/// Network APIs and clients no source file of the crate may name.
const NETWORK_APIS: [&str; 14] = [
    "std::net",
    "TcpStream",
    "UdpSocket",
    "TcpListener",
    "ToSocketAddrs",
    "SocketAddr",
    "tokio::net",
    "hyper",
    "reqwest",
    "ureq",
    "curl",
    "hf_hub",
    "http://",
    "https://",
];

/// Dependencies and features the manifest may not name (3.18.2(c)).
const MANIFEST_REFUSED: [&str; 9] = [
    "hyper",
    "reqwest",
    "ureq",
    "curl",
    "tokio",
    "hf-hub",
    "download-binaries",
    "copy-dylibs",
    "fetch-models",
];

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_source_file_names_a_network_api() {
    let mut files = Vec::new();
    rust_files(&crate_dir().join("src"), &mut files);
    assert!(files.len() >= 10, "the crate's sources: {files:?}");
    let mut found = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).unwrap();
        for api in NETWORK_APIS {
            if text.contains(api) {
                found.push(format!("{}: {api}", file.display()));
            }
        }
    }
    assert!(found.is_empty(), "network APIs in src: {found:?}");
}

#[test]
fn the_manifest_declares_no_network_client_hub_or_download_feature() {
    let manifest = fs::read_to_string(crate_dir().join("Cargo.toml")).unwrap();
    let declarations: String = manifest
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let found: Vec<_> = MANIFEST_REFUSED
        .iter()
        .filter(|name| declarations.contains(*name))
        .collect();
    assert!(found.is_empty(), "refused in Cargo.toml: {found:?}");
    assert!(
        declarations.contains("default-features = false, features = [\"std\", \"load-dynamic\"")
    );
}
