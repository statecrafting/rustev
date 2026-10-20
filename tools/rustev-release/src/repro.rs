//! Reproducible package verification (spec 021, 3.7): the comparison of two
//! independent packaging runs, and the test of each unpacked archive outside
//! the workspace.
//!
//! The unpacked test resolves dependencies only from a vendored directory
//! source: the exact candidate `.crate` archives (unpacked, with their
//! SHA-256 as the package checksum) and the workspace's locked third-party
//! sources from `cargo vendor`. There is no workspace manifest, path
//! override, Git source or `[patch]`, and nothing is fetched while testing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ReleaseSet;
use crate::archive::{UnpackedFile, sha256_hex, unpack};

/// One difference between two packaging runs of a package.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Difference {
    /// A run produced no archive for the package.
    Absent { package: String, run: char },
    /// A file is in one run's archive only.
    OnlyIn {
        package: String,
        run: char,
        path: String,
    },
    /// A file's bytes differ between the runs.
    Content { package: String, path: String },
    /// Every file is identical but the archives' bytes differ (tar or gzip
    /// metadata such as a timestamp or file mode).
    Container {
        package: String,
        a: String,
        b: String,
    },
}

impl std::fmt::Display for Difference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Difference::Absent { package, run } => {
                write!(f, "021 3.7: run {run} produced no archive of `{package}`")
            }
            Difference::OnlyIn { package, run, path } => {
                write!(f, "021 3.7: `{package}` `{path}` is only in run {run}")
            }
            Difference::Content { package, path } => {
                write!(f, "021 3.7: `{package}` `{path}` differs between the runs")
            }
            Difference::Container { package, a, b } => write!(
                f,
                "021 3.7: `{package}` files are identical but the archives differ ({a} and {b}): container metadata"
            ),
        }
    }
}

fn digests(files: &[UnpackedFile]) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|f| (f.path.clone(), sha256_hex(&f.bytes)))
        .collect()
}

/// Compare the `.crate` archives of every member in `a` and `b`. An empty
/// result means both runs produced byte-identical archives.
pub fn compare(
    set: &ReleaseSet,
    a: &Path,
    b: &Path,
    scratch: &Path,
) -> Result<Vec<Difference>, String> {
    let mut out = Vec::new();
    for name in &set.upload_order {
        let root = format!("{name}-{}", set.version);
        let file = format!("{root}.crate");
        let (pa, pb) = (a.join(&file), b.join(&file));
        let (ba, bb) = match (std::fs::read(&pa), std::fs::read(&pb)) {
            (Ok(x), Ok(y)) => (x, y),
            (ra, rb) => {
                for (run, missing) in [('a', ra.is_err()), ('b', rb.is_err())] {
                    if missing {
                        out.push(Difference::Absent {
                            package: name.clone(),
                            run,
                        });
                    }
                }
                continue;
            }
        };
        if ba == bb {
            continue;
        }
        let fa = digests(&unpack(&pa, &scratch.join("a").join(name), &root)?);
        let fb = digests(&unpack(&pb, &scratch.join("b").join(name), &root)?);
        let before = out.len();
        for (path, da) in &fa {
            match fb.get(path) {
                None => out.push(Difference::OnlyIn {
                    package: name.clone(),
                    run: 'a',
                    path: path.clone(),
                }),
                Some(db) if db != da => out.push(Difference::Content {
                    package: name.clone(),
                    path: path.clone(),
                }),
                Some(_) => {}
            }
        }
        for path in fb.keys().filter(|p| !fa.contains_key(*p)) {
            out.push(Difference::OnlyIn {
                package: name.clone(),
                run: 'b',
                path: path.clone(),
            });
        }
        if out.len() == before {
            out.push(Difference::Container {
                package: name.clone(),
                a: sha256_hex(&ba),
                b: sha256_hex(&bb),
            });
        }
    }
    out.sort();
    Ok(out)
}

/// A scratch directory removed when dropped, on every path including a
/// panic.
pub struct Scratch(pub PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

/// A new, empty scratch directory under the system temporary directory. Its
/// name is unpredictable (process, time and a counter), and it is created
/// with `create_dir`, which fails rather than follow anything already at
/// that path.
pub fn fresh_dir(prefix: &str) -> Result<Scratch, String> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}-{n}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(Scratch(dir))
}

/// The `.cargo-checksum.json` of a vendored package: every file's SHA-256
/// and the archive's SHA-256, the checksum a registry records for it.
pub fn checksum_json(files: &[UnpackedFile], archive_sha256: &str) -> String {
    let files: BTreeMap<&str, String> = files
        .iter()
        .map(|f| (f.path.as_str(), sha256_hex(&f.bytes)))
        .collect();
    serde_json::json!({ "files": files, "package": archive_sha256 }).to_string()
}

/// The source replacement every unpacked test uses: crates.io resolves only
/// to the vendored directory. crates.io is the only source to replace: the
/// manifest policy refuses any alternate registry or Git source in the
/// release graph (021 3.4, `check_manifests`), and `offline` makes any
/// other source a resolution failure rather than a fetch.
pub fn source_config(vendor: &Path) -> String {
    format!(
        "[source.crates-io]\nreplace-with = \"candidates\"\n\n[source.candidates]\ndirectory = {}\n\n[net]\noffline = true\n",
        toml_string(&vendor.to_string_lossy())
    )
}

/// A TOML basic string: quotes, backslashes and control characters
/// escaped, everything else (non-ASCII included) literal.
pub fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A command with no inherited Cargo or rustc configuration from the
/// invoking build, so a test cannot see the workspace through its
/// environment. `toolchain` selects the rustup toolchain. The vendor step
/// runs with the operator's Cargo home (it gathers the locked sources); the
/// unpacked test sets a fresh one (`isolated_home`).
pub fn clean_command(program: &str, toolchain: &str) -> Command {
    let mut c = Command::new(program);
    for (k, _) in std::env::vars_os() {
        let k = k.to_string_lossy().into_owned();
        // `CARGO_HOME` is the operator's choice of Cargo home and is kept;
        // the unpacked test replaces it with a fresh one.
        // `RUSTUP_HOME` likewise locates the operator's toolchains and is
        // kept; `RUSTUP_TOOLCHAIN` is set below.
        let keep = k == "CARGO_HOME" || k == "RUSTUP_HOME";
        if !keep && (k.starts_with("CARGO") || k.starts_with("RUST") || k == "OUT_DIR") {
            c.env_remove(&k);
        }
    }
    c.env("RUSTUP_TOOLCHAIN", toolchain);
    c
}

/// Lay out the vendored directory: `cargo vendor` of the workspace's locked
/// third-party sources, then each candidate archive unpacked beside them.
pub fn vendor(
    set: &ReleaseSet,
    workspace: &Path,
    crates: &Path,
    work: &Path,
    toolchain: &str,
) -> Result<PathBuf, String> {
    let vendor = work.join("vendor");
    let out = clean_command("cargo", toolchain)
        .args(["vendor", "--locked", "--versioned-dirs", "--quiet"])
        .arg(&vendor)
        .current_dir(workspace)
        .output()
        .map_err(|e| format!("could not run cargo vendor: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo vendor failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    for name in &set.upload_order {
        let root = format!("{name}-{}", set.version);
        let archive = crates.join(format!("{root}.crate"));
        let bytes = std::fs::read(&archive).map_err(|e| format!("{}: {e}", archive.display()))?;
        let _ = std::fs::remove_dir_all(vendor.join(&root));
        let files = stage(&archive, work, &root, &vendor)?;
        std::fs::write(
            vendor.join(&root).join(".cargo-checksum.json"),
            checksum_json(&files, &sha256_hex(&bytes)),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(vendor)
}

/// Unpack `archive` into an empty staging directory, refuse anything beside
/// or above the package root, then move the root into `dest`. `unpack`
/// already refuses unsafe entries before extracting (archive.rs,
/// `check_entries`); this keeps the destination's other contents out of the
/// archive's view.
fn stage(
    archive: &Path,
    work: &Path,
    root: &str,
    dest: &Path,
) -> Result<Vec<UnpackedFile>, String> {
    let staging = work.join("stage").join(root);
    let _ = std::fs::remove_dir_all(&staging);
    // `unpack` lists the archive and refuses absolute, `..` and link entries
    // before tar writes anything, and extraction lands only in this fresh
    // staging directory. An archive must also hold its package root alone:
    // anything else at the top of the staging directory is refused before
    // the root is moved into place.
    let files = unpack(archive, &staging, root)?;
    let top: Vec<String> = std::fs::read_dir(&staging)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    if top != [root] {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!(
            "{} holds {top:?} beside {root}/",
            archive.display()
        ));
    }
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    std::fs::rename(staging.join(root), dest.join(root)).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(files)
}

/// Test one unpacked archive outside the workspace: a fresh copy of the
/// package root under `work/test/`, the vendored source as its only
/// registry, offline, with the archive's own lockfile.
pub fn test_unpacked(
    name: &str,
    version: &str,
    crates: &Path,
    vendor: &Path,
    work: &Path,
    toolchain: &str,
) -> Result<std::process::Output, String> {
    let root = format!("{name}-{version}");
    let tests = work.join("test");
    std::fs::create_dir_all(tests.join(".cargo")).map_err(|e| e.to_string())?;
    std::fs::write(tests.join(".cargo/config.toml"), source_config(vendor))
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(tests.join(&root));
    stage(&crates.join(format!("{root}.crate")), work, &root, &tests)?;
    // A fresh, empty Cargo home: no registry cache, credentials or config of
    // the operator's can take part; the vendored directory is the only source.
    let isolated_home = work.join("cargo-home");
    std::fs::create_dir_all(&isolated_home).map_err(|e| e.to_string())?;
    clean_command("cargo", toolchain)
        .env("CARGO_HOME", &isolated_home)
        // One target directory for the eight tests: they share the vendored
        // third-party crates, and Cargo keys every unit by package id and
        // source path, so one package's artifacts never stand in for
        // another's. The directory is new for each candidate run.
        .args(["test", "--locked", "--offline", "--quiet", "--target-dir"])
        .arg(work.join("target"))
        .current_dir(tests.join(&root))
        .output()
        .map_err(|e| format!("could not run cargo test: {e}"))
}
