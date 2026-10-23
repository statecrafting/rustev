//! `rustev-release`: release-candidate checks of spec 021. Read-only: it
//! never uploads, tags, signs or reads a credential.
//!
//! `rustev-release manifests` runs `cargo metadata` for this workspace and
//! exits 1 naming every violation of the release set's manifest policy,
//! and of the npm members' `package.json` (spec 031 3.1).
//!
//! `rustev-release archives <dir>` unpacks each member's `.crate` from
//! `<dir>` (as `cargo package` writes them), prints the archive evidence as
//! JSON and exits 1 naming every violation of the archive policy (3.5).
//!
//! `rustev-release compare <dir-a> <dir-b>` exits 1 naming every difference
//! between two packaging runs' archives (3.7).
//!
//! `rustev-release unpacked <dir> <work> [<toolchain>]` tests each member's
//! unpacked archive outside any workspace, offline, against a vendored
//! source of the candidate archives and the locked third-party crates
//! (3.7). `<work>` must not lie inside a Cargo workspace.
//!
//! `rustev-release deps` prints the dependency report of 3.6 (normal and
//! build closure, development-only additions, each with source, checksum
//! and license) and exits 1 on a missing license or checksum.
//!
//! `rustev-release manifest <evidence-dir>` writes the canonical
//! `release-manifest.json` and `SHA256SUMS` for the archives in
//! `<evidence-dir>/a` at the clean commit at HEAD (3.8). Signing them and
//! the tag is the owner's act.
//!
//! `rustev-release verify-release <dir> [<repo>]` checks every binding of
//! 3.8 against the committed `allowed_signers` and the owner identity.
//!
//! `rustev-release publish <release-dir> <instruction.json> <journal>` is the
//! owner-authorized publication of 3.9: it verifies the signed release in
//! `<release-dir>` against the local tag, checks the instruction names
//! exactly that release (I-4), records the account and release identity in
//! the journal (a file outside the repository), and uploads the exact
//! verified archives to crates.io in order. It reads `CARGO_REGISTRY_TOKEN`
//! only for the account query and the uploads.
//!
//! `rustev-release changelog <out.md> [<previous tag>]` writes the release
//! changelog generated from the governed corpus (spec 031, 3.10) through
//! `spec-spine registry list --json` at the clean commit at HEAD and, when a
//! previous release is named, at that tag. It reads only the repository.
//!
//! `rustev-release exceptions <YYYY-MM-DD>` exits 1 unless every advisory
//! `deny.toml` ignores has an owner-approved exception unexpired on that
//! date (3.6).

use std::path::Path;
use std::process::{Command, ExitCode};

use rustev_release::archive::{ArchivePolicy, evidence, unpack_bytes};
use rustev_release::{Metadata, ReleaseSet, check_manifests};

const SET: &str = include_str!("../release-set.json");
const POLICY: &str = include_str!("../archive-policy.json");
const DENY: &str = include_str!("../deny.toml");
const EXCEPTIONS: &str = include_str!("../advisory-exceptions.json");
const LOCK: &str = include_str!("../../../Cargo.lock");

fn metadata() -> Result<Metadata, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .map_err(|e| format!("could not run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("unreadable cargo metadata: {e}"))
}

fn manifests(set: &ReleaseSet) -> ExitCode {
    let meta = match metadata() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut violations: Vec<String> = check_manifests(set, &meta)
        .iter()
        .map(ToString::to_string)
        .collect();
    // Spec 031 3.1: the npm members' manifests, read from this checkout.
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    violations.extend(
        rustev_release::npm::check_npm(set, |p| {
            std::fs::read(root.join(p)).map_err(|e| format!("{p}: {e}"))
        })
        .iter()
        .map(ToString::to_string),
    );
    if violations.is_empty() {
        println!(
            "release set {}: {} crate(s) pass the manifest policy of spec 021 and {} npm package(s) that of spec 031 3.1 (candidate evidence only)",
            set.version,
            set.upload_order.len(),
            set.npm.len()
        );
        ExitCode::SUCCESS
    } else {
        for v in &violations {
            eprintln!("{v}");
        }
        ExitCode::from(1)
    }
}

fn archives(set: &ReleaseSet, dir: &str) -> ExitCode {
    let policy = match ArchivePolicy::parse(POLICY.as_bytes()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let scratch = std::env::temp_dir().join(format!("rustev-release-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    // Every path, failing or not, removes the scratch directory below.
    let result = (|| -> Result<Vec<_>, String> {
        let mut report = Vec::new();
        for name in &set.upload_order {
            let root = format!("{name}-{}", set.version);
            let path = std::path::Path::new(dir).join(format!("{root}.crate"));
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let files = unpack_bytes(&bytes, &path, &scratch.join(name), &root)?;
            report.push(evidence(&policy, name, &set.version, &bytes, &files));
        }
        Ok(report)
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    let report = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut failed = false;
    for v in report.iter().flat_map(|ev| &ev.violations) {
        eprintln!("{v}");
        failed = true;
    }
    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn compare(set: &ReleaseSet, a: &str, b: &str) -> ExitCode {
    let scratch = match rustev_release::repro::fresh_dir("rustev-release-cmp") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let result = rustev_release::repro::compare(set, Path::new(a), Path::new(b), &scratch);
    // `scratch` is removed when it drops, on every path.
    match result {
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
        Ok(d) if d.is_empty() => {
            println!(
                "{} archive(s) byte-identical between the two runs",
                set.upload_order.len()
            );
            ExitCode::SUCCESS
        }
        Ok(d) => {
            for x in &d {
                // The differences are the evidence: stdout, like the
                // success line, so a caller capturing stdout sees them.
                println!("{x}");
            }
            ExitCode::from(1)
        }
    }
}

/// The development toolchain pinned by the workspace's `rust-toolchain.toml`.
fn pinned_toolchain() -> Option<String> {
    let text = include_str!("../../../rust-toolchain.toml");
    // Exactly `channel = "<value>"`: the key alone, a double-quoted value.
    text.lines().find_map(|l| {
        let (key, value) = l.split_once('=')?;
        if key.trim() != "channel" {
            return None;
        }
        let value = value.trim();
        let inner = value.strip_prefix('"')?.split('"').next()?;
        (!inner.is_empty()).then(|| inner.to_string())
    })
}

fn unpacked(set: &ReleaseSet, dir: &str, work: &str, toolchain: Option<&str>) -> ExitCode {
    let work = Path::new(work);
    // Refuse a path inside a workspace before creating anything there: the
    // nearest existing ancestor is resolved (symlinks and `..` included), and
    // the part below it does not exist yet, so it holds no `Cargo.toml`.
    let absolute = std::env::current_dir()
        .map(|d| d.join(work))
        .unwrap_or_else(|_| work.to_path_buf());
    let existing = absolute
        .ancestors()
        .find(|a| a.exists())
        .and_then(|a| a.canonicalize().ok());
    if existing.is_some_and(|e| e.ancestors().any(|a| a.join("Cargo.toml").exists())) {
        eprintln!(
            "{} lies inside a Cargo workspace; use a directory outside it (3.7)",
            work.display()
        );
        return ExitCode::from(2);
    }
    // The outermost directory this command creates, if any.
    let first_new = work
        .ancestors()
        .take_while(|a| !a.exists())
        .last()
        .map(Path::to_path_buf);
    if let Err(e) = std::fs::create_dir_all(work) {
        eprintln!("{}: {e}", work.display());
        return ExitCode::from(2);
    }
    let work = match work.canonicalize() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    if work.ancestors().any(|a| a.join("Cargo.toml").exists()) {
        // Remove what this command created rather than leave it behind.
        if let Some(d) = &first_new {
            let _ = std::fs::remove_dir_all(d);
        }
        eprintln!(
            "{} lies inside a Cargo workspace; use a directory outside it (3.7)",
            work.display()
        );
        return ExitCode::from(2);
    }
    let Some(toolchain) = toolchain.map(str::to_string).or_else(pinned_toolchain) else {
        eprintln!("no toolchain given and none pinned");
        return ExitCode::from(2);
    };
    let crates = match Path::new(dir).canonicalize() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{dir}: {e}");
            return ExitCode::from(2);
        }
    };
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let vendor = match rustev_release::repro::vendor(set, &workspace, &crates, &work, &toolchain) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut failed = false;
    for name in &set.upload_order {
        match rustev_release::repro::test_unpacked(
            name,
            &set.version,
            &crates,
            &vendor,
            &work,
            &toolchain,
        ) {
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
            Ok(o) if o.status.success() => {
                println!("{name} {}: unpacked tests pass on {toolchain}", set.version)
            }
            Ok(o) => {
                failed = true;
                eprintln!(
                    "021 3.7: `{name}` unpacked tests fail on {toolchain}:\n{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
            }
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// One value the candidate script needs, printed alone; refused unless it
/// is plain (no quote, space or control character), so the shell never
/// parses JSON or receives a value it could split or expand.
fn field(set: &ReleaseSet, name: &str) -> ExitCode {
    let values: Vec<String> = match name {
        "version" => vec![set.version.clone()],
        "packages" => set.upload_order.clone(),
        "excluded" => set.excluded.iter().map(|e| e.name.clone()).collect(),
        "msrv" => match metadata().map(|m| {
            m.packages
                .into_iter()
                .find(|p| Some(&p.name) == set.upload_order.first())
                .and_then(|p| p.rust_version)
        }) {
            Ok(Some(v)) => vec![v],
            Ok(None) => {
                eprintln!("no rust-version declared");
                return ExitCode::from(2);
            }
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
        },
        "target" => match target_directory() {
            Ok(t) => vec![t],
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
        },
        _ => {
            eprintln!("field is version, packages, excluded, msrv or target");
            return ExitCode::from(2);
        }
    };
    for v in &values {
        // A target directory is a path and may hold spaces; every other
        // field is a plain word the script may split. No value may hold a
        // control character (a newline would end it early).
        let plain = |b: u8| b.is_ascii_alphanumeric() || b"._/-+@".contains(&b);
        let ok = !v.is_empty()
            && !v.chars().any(char::is_control)
            && (name == "target" || v.bytes().all(plain));
        if !ok {
            eprintln!("refusing the {name} value {v:?}");
            return ExitCode::from(2);
        }
        println!("{v}");
    }
    ExitCode::SUCCESS
}

fn target_directory() -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Meta {
        target_directory: String,
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .output()
        .map_err(|e| format!("could not run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    serde_json::from_slice::<Meta>(&out.stdout)
        .map(|m| m.target_directory)
        .map_err(|e| format!("unreadable cargo metadata: {e}"))
}

fn deps(set: &ReleaseSet) -> ExitCode {
    let meta = match metadata() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let report = rustev_release::audit::report(set, &meta, LOCK);
    for p in &report.problems {
        eprintln!("{p}");
    }
    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    }
    if report.problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Today's UTC date as `YYYY-MM-DD`, from the system clock.
fn utc_today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rustev_release::audit::civil_date(secs)
}

fn exceptions(today: &str) -> ExitCode {
    // Expiry is judged against the real date: the argument must be a
    // `YYYY-MM-DD` date and today's UTC date, so a later date cannot pass an
    // expired exception.
    let well_formed = today.len() == 10
        && today.char_indices().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        });
    // One reading of the clock, for the check and its message alike.
    let utc = utc_today();
    if !well_formed || today != utc {
        eprintln!(
            "021 3.6: exceptions takes today's UTC date as YYYY-MM-DD ({utc}), not {today:?}"
        );
        return ExitCode::from(2);
    }
    let ex = match rustev_release::audit::Exceptions::parse(EXCEPTIONS.as_bytes()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let ignored = rustev_release::audit::deny_ignored(DENY);
    let v = rustev_release::audit::check_exceptions(&ex, &ignored, today);
    if v.is_empty() {
        println!(
            "{} advisory exception(s), each owner-approved and unexpired on {today}",
            ex.exceptions.len()
        );
        ExitCode::SUCCESS
    } else {
        for x in &v {
            eprintln!("{x}");
        }
        ExitCode::from(1)
    }
}

/// The workspace's spec-spine (`.bin/spec-spine`, which `make tools`
/// installs, or `SPEC_SPINE`), accepted only at the exact version
/// `spec-spine.toml` pins, so no other binary can answer for the corpus.
fn spec_spine() -> Result<String, String> {
    let bin = std::env::var("SPEC_SPINE").unwrap_or_else(|_| ".bin/spec-spine".to_string());
    let toml =
        std::fs::read_to_string("spec-spine.toml").map_err(|e| format!("spec-spine.toml: {e}"))?;
    let pin = toml
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == "required_version").then(|| {
                v.trim()
                    .trim_matches('"')
                    .trim_start_matches('=')
                    .to_string()
            })
        })
        .ok_or("spec-spine.toml pins no required_version")?;
    let version = command_line(&bin, &["--version"])?;
    if version != format!("spec-spine {pin}") {
        return Err(format!(
            "{bin} is `{version}`, not the pinned spec-spine {pin}"
        ));
    }
    Ok(bin)
}

/// The registry listing of the corpus at `repo`.
fn registry(repo: &str) -> Result<Vec<rustev_release::changelog::Spec>, String> {
    let text = command_line(
        &spec_spine()?,
        &["--repo", repo, "registry", "list", "--json"],
    )?;
    serde_json::from_str::<rustev_release::changelog::Registry>(&text)
        .map(|r| r.items)
        .map_err(|e| format!("unreadable registry listing at {repo}: {e}"))
}

fn changelog_cmd(set: &ReleaseSet, out: &str, previous: Option<&str>) -> ExitCode {
    match changelog(set, previous) {
        Ok(text) => {
            if let Err(e) = std::fs::write(out, &text) {
                eprintln!("{out}: {e}");
                return ExitCode::from(1);
            }
            let digest = rustev_release::archive::sha256_hex(text.as_bytes());
            println!("changelog {out}: sha256 {digest}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("031 3.10: {e}");
            ExitCode::from(1)
        }
    }
}

fn changelog(set: &ReleaseSet, previous: Option<&str>) -> Result<String, String> {
    use rustev_release::changelog::{Inputs, PlanIdentity, approvals, render};
    if !command_line("git", &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        return Err("the changelog is generated at a clean commit; the tree is dirty".into());
    }
    // `command_line` returns trimmed output, so no newline reaches the text.
    let commit = command_line("git", &["rev-parse", "HEAD"])?;
    let after = registry(".")?;
    // The previous release's corpus, read from its tag's tree.
    let before = match previous {
        None => Vec::new(),
        Some(tag) => {
            // A fresh private directory (`mktemp -d`, mode 0700), never a
            // predictable or reused path.
            let dir = std::path::PathBuf::from(command_line(
                "mktemp",
                &[
                    "-d",
                    &format!("{}/rustev-changelog.XXXXXX", std::env::temp_dir().display()),
                ],
            )?);
            let dir_s = dir.display().to_string();
            let archive = dir.join("tree.tar").display().to_string();
            // Every step's result is taken before the directory is removed,
            // so a failing `git archive` or `tar` leaves nothing behind.
            let r = command_line("git", &["archive", "--output", &archive, tag])
                .and_then(|_| command_line("tar", &["-xf", &archive, "-C", &dir_s]))
                .and_then(|_| registry(&dir_s));
            let _ = std::fs::remove_dir_all(&dir);
            r?
        }
    };
    let decisions = std::fs::read_to_string("docs/decisions/00-founding-decisions.md")
        .map_err(|e| format!("decisions: {e}"))?;
    let range = previous.map_or("HEAD".to_string(), |p| format!("{p}..HEAD"));
    let mut commits = std::collections::BTreeMap::new();
    for s in &after {
        let log = command_line(
            "git",
            &[
                "log",
                "--reverse",
                "--format=%h%x09%s",
                &range,
                "--",
                &format!("specs/{}/", s.id),
            ],
        )?;
        let list: Vec<(String, String)> = log
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(h, s)| (h.to_string(), s.to_string()))
            .collect();
        commits.insert(s.id.clone(), list);
    }
    let meta = metadata()?;
    let first = meta
        .packages
        .iter()
        .find(|p| Some(&p.name) == set.upload_order.first())
        .ok_or("the first release member is not in the workspace")?;
    // One MSRV and edition for the family: the release policy refuses a
    // member that differs (021 3.3), and the changelog states it only when
    // every member agrees.
    for p in meta
        .packages
        .iter()
        .filter(|p| set.members().any(|m| m == p.name))
    {
        if p.rust_version != first.rust_version || p.edition != first.edition {
            return Err(format!(
                "`{}` declares rust-version {:?} and edition {}, not the family's {:?} and {}",
                p.name, p.rust_version, p.edition, first.rust_version, first.edition
            ));
        }
    }
    // Each reference plan's identity, as its `plan check` golden pins it.
    let mut plan_ids = Vec::new();
    let golden = "crates/rustev-cli/tests/golden";
    let mut names: Vec<String> = std::fs::read_dir(golden)
        .map_err(|e| format!("{golden}: {e}"))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".check.json") && !n.contains("refus"))
        .collect();
    names.sort();
    let plan_id = |text: &str| -> Option<String> {
        serde_json::from_str::<serde_json::Value>(text).ok()?["plan_id"]
            .as_str()
            .map(str::to_string)
    };
    for n in names {
        let path = format!("{golden}/{n}");
        let now = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
        // A `plan check` golden always carries a `plan_id`; one without it
        // is an error, here as at the previous release.
        let after = plan_id(&now).ok_or_else(|| format!("{path}: no plan_id"))?;
        // Absent at the previous release is `None`; present but unreadable
        // is an error, never a silent "unchanged".
        let before = match previous {
            None => None,
            Some(p) => {
                let at = format!("{p}:{path}");
                if command_line("git", &["cat-file", "-e", &at]).is_err() {
                    None
                } else {
                    let text = command_line("git", &["show", &at])?;
                    Some(plan_id(&text).ok_or_else(|| format!("{at}: no plan_id"))?)
                }
            }
        };
        plan_ids.push(PlanIdentity {
            plan: n.trim_end_matches(".check.json").to_string(),
            before,
            after,
        });
    }
    let inputs = Inputs {
        version: set.version.clone(),
        previous: previous.map(str::to_string),
        commit,
        approvals: approvals(&decisions, &after),
        before,
        after,
        commits,
        msrv: first
            .rust_version
            .clone()
            .unwrap_or_else(|| "unknown".into()),
        edition: first.edition.clone(),
        plan_ids,
        superseded: Vec::new(),
    };
    Ok(render(&inputs))
}

fn command_line(program: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(program);
    // No inherited `GIT_*` variable (such as `GIT_DIR`) may point the dirty
    // check or the recorded commit and tree at another repository.
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(&k);
        }
    }
    let out = cmd
        .args(args)
        .output()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn manifest(set: &ReleaseSet, evidence_dir: &str) -> ExitCode {
    use rustev_release::archive::sha256_hex;
    use rustev_release::manifest::{Inputs, Toolchain, build, sha256sums};
    let dir = Path::new(evidence_dir);
    let result = (|| -> Result<(), String> {
        if !command_line("git", &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
            return Err(
                "021 3.8: the manifest names an exact clean commit; the tree is dirty".into(),
            );
        }
        let commit = command_line("git", &["rev-parse", "HEAD"])?;
        // Clean and at the same commit after reading it, so nothing changed
        // between the check above and the commit recorded.
        if !command_line("git", &["status", "--porcelain", "--untracked-files=no"])?.is_empty()
            || command_line("git", &["rev-parse", "HEAD"])? != commit
        {
            return Err("021 3.8: the tree changed while the manifest was being built".into());
        }
        let tree = command_line("git", &["rev-parse", &format!("{commit}^{{tree}}")])?;
        let epoch: u64 = command_line("git", &["log", "-1", "--format=%ct", &commit])?
            .parse()
            .map_err(|e| format!("commit time: {e}"))?;
        let rustc = command_line("rustc", &["-vV"])?;
        let host = rustc
            .lines()
            .find_map(|l| l.strip_prefix("host: "))
            .unwrap_or("unknown")
            .to_string();
        let mut evidence = std::collections::BTreeMap::new();
        for name in [
            "compare.txt",
            "archives.json",
            "unpacked.txt",
            "deny.txt",
            "exceptions.txt",
            "dependencies.json",
        ] {
            let bytes = std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
            evidence.insert(name.to_string(), sha256_hex(&bytes));
        }
        // Spec 031 3.9.1 and 3.10: every member's SBOM and the changelog are
        // release artifacts whose digests the manifest carries; a missing one
        // fails the candidate.
        let sboms = set
            .upload_order
            .iter()
            .map(|n| (format!("sbom-{n}.cdx.json"), "031 3.9.1"));
        for (name, clause) in sboms.chain([("changelog.md".to_string(), "031 3.10")]) {
            let bytes =
                std::fs::read(dir.join(&name)).map_err(|e| format!("{clause}: {name}: {e}"))?;
            evidence.insert(name, sha256_hex(&bytes));
        }
        let m = build(
            set,
            &dir.join("a"),
            Inputs {
                commit,
                tree,
                toolchain: Toolchain {
                    rustc: rustc.lines().next().unwrap_or_default().to_string(),
                    cargo: command_line("cargo", &["--version"])?,
                    host,
                    audit: command_line("cargo", &["deny", "--version"])?,
                },
                source_date_epoch: epoch,
                evidence,
                limitations: LIMITATIONS.iter().map(|l| l.to_string()).collect(),
            },
        )?;
        // Both files are written in full beside their targets, then renamed
        // (each rename atomic), the manifest last: a manifest present means
        // its SHA256SUMS is complete, and an interrupted run leaves only
        // `.partial` files, which a re-run overwrites.
        let put = |name: &str, bytes: &[u8]| -> Result<(), String> {
            let partial = dir.join(format!("{name}.partial"));
            std::fs::write(&partial, bytes).map_err(|e| format!("{}: {e}", partial.display()))
        };
        put("SHA256SUMS", sha256sums(&m).as_bytes())?;
        put("release-manifest.json", &m.canonical())?;
        for name in ["SHA256SUMS", "release-manifest.json"] {
            std::fs::rename(dir.join(format!("{name}.partial")), dir.join(name))
                .map_err(|e| format!("{name}: {e}"))?;
        }
        println!("release-manifest.json sha256 {}", m.digest());
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

/// Every limitation the first release states (3.8); the release notes
/// repeat them.
const LIMITATIONS: &[&str] = &[
    "Source archives only: no prebuilt binary, container or npm package.",
    "Supported targets are those the candidate matrix tested; others are unsupported.",
    "Synthetic reference datasets and reports support no quality or calibration claim.",
    "Pre-1.0: a minor version may change any public contract (3.2).",
];

fn verify_release_cmd(dir: &str, repo: &str) -> ExitCode {
    // The built-in record, written to a scratch file `ssh-keygen` can read.
    let scratch = match rustev_release::repro::fresh_dir("rustev-release-signers") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    // `scratch` is a `Scratch` guard: the directory is removed when it drops
    // at the end of this function, on every path.
    let signers = scratch.join("allowed_signers");
    if let Err(e) = std::fs::write(&signers, rustev_release::manifest::ALLOWED_SIGNERS) {
        eprintln!("{}: {e}", signers.display());
        return ExitCode::from(2);
    }
    let v = rustev_release::manifest::verify_release(
        Path::new(dir),
        Path::new(repo),
        &signers,
        rustev_release::manifest::OWNER_IDENTITY,
    );
    if v.is_empty() {
        println!("release in {dir}: commit, tree, tag, manifest, checksums and key bind (3.8)");
        ExitCode::SUCCESS
    } else {
        for x in &v {
            eprintln!("{x}");
        }
        ExitCode::from(1)
    }
}

fn publish_cmd(dir: &str, instruction: &str, journal_path: &str) -> ExitCode {
    use rustev_release::cratesio::CratesIo;
    use rustev_release::lifecycle::{
        Entry, Event, Instruction, Journal, Lifecycle, Outcome, publish,
    };
    use rustev_release::manifest::ReleaseManifest;
    let result = (|| -> Result<Outcome, String> {
        let dir = Path::new(dir);
        let journal_path = Path::new(journal_path);
        // The journal lives outside Git (3.9).
        let top = command_line("git", &["rev-parse", "--show-toplevel"])?;
        // Both sides canonical, so a symlink cannot hide the journal inside.
        let top = Path::new(&top)
            .canonicalize()
            .map_err(|e| format!("{top}: {e}"))?;
        let parent = journal_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = parent
            .canonicalize()
            .map_err(|e| format!("{}: {e}", parent.display()))?;
        if parent.starts_with(&top) {
            return Err(
                "021 3.9: the publication journal is inside the repository; it must live outside it"
                    .into(),
            );
        }
        // The signed release: archives, manifest, checksums, tag and key bind.
        let scratch = rustev_release::repro::fresh_dir("rustev-release-signers")?;
        let signers = scratch.join("allowed_signers");
        std::fs::write(&signers, rustev_release::manifest::ALLOWED_SIGNERS)
            .map_err(|e| e.to_string())?;
        let v = rustev_release::manifest::verify_release(
            dir,
            &top,
            &signers,
            rustev_release::manifest::OWNER_IDENTITY,
        );
        if !v.is_empty() {
            return Err(v.join("\n"));
        }
        let bytes = std::fs::read(dir.join("release-manifest.json")).map_err(|e| e.to_string())?;
        let m = ReleaseManifest::parse(&bytes)?;
        // The candidate evidence the manifest names is the evidence present.
        for (name, digest) in &m.evidence {
            let b = std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
            if rustev_release::archive::sha256_hex(&b) != *digest {
                return Err(format!(
                    "021 3.7: evidence {name} is not the one the manifest names"
                ));
            }
        }
        let instruction: Instruction = serde_json::from_slice(
            &std::fs::read(instruction).map_err(|e| format!("{instruction}: {e}"))?,
        )
        .map_err(|e| format!("unreadable instruction: {e}"))?;
        let mut life = Lifecycle::new(m.clone());
        life.apply(Event::GatesPass).map_err(|r| r.0)?;
        life.apply(Event::CandidateVerified {
            commit: m.commit.clone(),
        })
        .map_err(|r| r.0)?;
        life.apply(Event::Authorized(instruction))
            .map_err(|r| r.0)?;
        let mut journal = match std::fs::read_to_string(journal_path) {
            Ok(text) => Journal::from_lines(&text)?,
            // Only an absent journal starts empty; any other read failure
            // would discard the history the publisher resumes from.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Journal::default(),
            Err(e) => return Err(format!("journal {}: {e}", journal_path.display())),
        };
        journal.file = Some(journal_path.to_path_buf());
        let registry = CratesIo { wait_seconds: 10 };
        let account = registry.account()?;
        journal.record(Entry::Preflight {
            account,
            commit: m.commit.clone(),
            tag: m.tag.clone(),
            manifest_sha256: m.digest(),
        });
        let mut archives = std::collections::BTreeMap::new();
        for p in &m.packages {
            let b = std::fs::read(dir.join(&p.crate_file)).map_err(|e| e.to_string())?;
            archives.insert(p.crate_file.clone(), b);
        }
        let mut registry = registry;
        let outcome = publish(&mut life, &archives, &mut registry, &mut journal);
        println!("lifecycle state: {:?}", life.state);
        Ok(outcome)
    })();
    match result {
        Ok(rustev_release::lifecycle::Outcome::Complete) => {
            println!(
                "every package is public with its verified checksum (published, not yet released or qualified)"
            );
            ExitCode::SUCCESS
        }
        Ok(rustev_release::lifecycle::Outcome::Stopped { reason }) => {
            eprintln!("publication stopped: {reason}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

// 031 3.9.1 to 3.9.3: artifact commands emit evidence and never publish.
fn artifact_cmd(set: &ReleaseSet, args: &[&str]) -> ExitCode {
    let result = (|| -> Result<bool, String> {
        let problems = match args {
            ["compare-artifacts", a, b] => {
                let report = rustev_release::artifacts::compare(Path::new(a), Path::new(b))?;
                println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
                report.problems
            }
            ["npm-audit", dir] => rustev_release::artifacts::npm_audit(set, Path::new(dir))?,
            ["sbom-check", dir, sboms] => rustev_release::artifacts::sbom_check(Path::new(dir), Path::new(sboms))?,
            ["main-sbom", tarball, epoch] => {
                let epoch = epoch.parse::<u64>().map_err(|e| e.to_string())?;
                let bom = rustev_release::artifacts::main_sbom(set, Path::new(tarball), epoch)?;
                println!("{}", serde_json::to_string_pretty(&bom).map_err(|e| e.to_string())?);
                vec![]
            }
            _ => return Err("usage: compare-artifacts <a> <b> | npm-audit <dir> | sbom-check <artifacts> <sboms> | main-sbom <tarball> <epoch>".into()),
        };
        for problem in &problems {
            eprintln!("{problem}");
        }
        Ok(problems.is_empty())
    })();
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("031 3.9: {e}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let set = match ReleaseSet::parse(SET.as_bytes()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [
            "compare-artifacts" | "npm-audit" | "sbom-check" | "main-sbom",
            ..,
        ] => artifact_cmd(&set, &args.iter().map(String::as_str).collect::<Vec<_>>()),
        ["manifests"] => manifests(&set),
        ["archives", dir] => archives(&set, dir),
        ["compare", a, b] => compare(&set, a, b),
        ["field", name] => field(&set, name),
        ["deps"] => deps(&set),
        ["publish", dir, instruction, journal] => publish_cmd(dir, instruction, journal),
        ["manifest", dir] => manifest(&set, dir),
        ["verify-release", dir] => verify_release_cmd(dir, "."),
        ["verify-release", dir, repo] => verify_release_cmd(dir, repo),
        ["exceptions", today] => exceptions(today),
        ["changelog", out] => changelog_cmd(&set, out, None),
        ["changelog", out, previous] => changelog_cmd(&set, out, Some(previous)),
        ["unpacked", dir, work] => unpacked(&set, dir, work, None),
        ["unpacked", dir, work, toolchain] => unpacked(&set, dir, work, Some(toolchain)),
        _ => {
            eprintln!(
                "usage: rustev-release manifests | archives <dir> | compare <dir-a> <dir-b> | field <name> | unpacked <dir> <work> [<toolchain>] | deps | exceptions <YYYY-MM-DD> | changelog <out.md> [<previous tag>] | manifest <evidence-dir> | publish <release-dir> <instruction.json> <journal> | verify-release <dir> [<repo>]"
            );
            ExitCode::from(2)
        }
    }
}
