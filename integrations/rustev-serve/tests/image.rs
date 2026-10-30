//! Static checks of the image definition against spec 042 3.16 and the
//! base image row of spec 010's rustev-serve binding record. The built image
//! itself is checked by `image/check.sh` (non-root, entrypoint, no shell).
use std::path::Path;

fn read(relative: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(relative)).unwrap_or_else(|e| panic!("{relative}: {e}"))
}

/// The Dockerfile's instructions, comments and continuations removed.
fn instructions() -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in read("integrations/rustev-serve/image/Dockerfile").lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line.strip_suffix('\\') {
            Some(head) => {
                current.push_str(head.trim_end());
                current.push(' ');
            }
            None => {
                current.push_str(line);
                out.push(std::mem::take(&mut current));
            }
        }
    }
    assert!(current.is_empty(), "dangling continuation");
    out
}

fn stages() -> Vec<Vec<String>> {
    let mut stages: Vec<Vec<String>> = Vec::new();
    for instruction in instructions() {
        if instruction.starts_with("FROM ") {
            stages.push(Vec::new());
        }
        stages
            .last_mut()
            .expect("the first instruction is FROM")
            .push(instruction);
    }
    stages
}

fn image_of(from: &str) -> &str {
    from.split_whitespace().nth(1).expect("FROM names an image")
}

fn digest_of(image: &str) -> &str {
    let (_, digest) = image
        .split_once("@sha256:")
        .unwrap_or_else(|| panic!("{image} is not pinned by digest"));
    assert_eq!(digest.len(), 64, "{image}");
    assert!(
        digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    digest
}

#[test]
fn every_base_is_pinned_by_digest() {
    let stages = stages();
    assert_eq!(stages.len(), 2, "a builder stage and the runtime stage");
    for stage in &stages {
        digest_of(image_of(&stage[0]));
    }
}

#[test]
fn runtime_base_is_the_binding_records_distroless_index() {
    let stages = stages();
    let runtime = image_of(&stages[1][0]);
    assert!(
        runtime.starts_with("gcr.io/distroless/cc-debian12:nonroot@sha256:"),
        "{runtime}"
    );
    let digest = digest_of(runtime);
    let record = read("specs/010-integrations/spec.md");
    let row = record
        .lines()
        .find(|line| line.starts_with("| 042 2.2: base image |"))
        .expect("the binding record's base image row");
    assert!(
        row.contains(&format!("index `sha256:{digest}`")),
        "the runtime base is not the recorded index: {row}"
    );
}

#[test]
fn builder_is_the_release_tools_linux_gnu_toolchain() {
    let stages = stages();
    let builder = image_of(&stages[0][0]);
    let artifacts = read("tools/rustev-release/artifacts.sh");
    assert!(
        artifacts.contains(&format!("*-gnu) image='{builder}'")),
        "the builder is not artifacts.sh's linux gnu image: {builder}"
    );
}

#[test]
fn runtime_stage_holds_the_binary_only_as_non_root() {
    let stages = stages();
    let runtime = &stages[1][1..];
    assert_eq!(
        runtime,
        [
            "COPY --from=build --chown=0:0 /out/rustev-serve /usr/local/bin/rustev-serve",
            "USER 65532:65532",
            r#"ENTRYPOINT ["/usr/local/bin/rustev-serve", "--config", "/etc/rustev/serve.json"]"#,
        ],
        "the runtime stage copies the binary only, runs no command and sets the fixed entrypoint"
    );
}

#[test]
fn builder_builds_the_locked_binary() {
    let stages = stages();
    assert!(
        stages[0].iter().any(|i| i
            .starts_with("RUN cargo build -p rustev-serve --bin rustev-serve --locked --release ")),
        "{:?}",
        stages[0]
    );
}
