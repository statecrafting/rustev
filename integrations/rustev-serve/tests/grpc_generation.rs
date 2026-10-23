use rustev_contract::serve::operation_table;
// `sha2` is a normal dependency of rustev-serve (Cargo.toml), so tests see it.
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[test]
fn committed_grpc_declarations_are_current() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = root
        .join(".grpc-generation")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&scratch).unwrap();
    let mut proto = String::from(
        "// Generated from rustev-contract operation_table. Do not edit.\nsyntax = \"proto3\";\npackage rustev.serve.v1;\n\n// Path parameters: rustev-plan-id and rustev-decision-id metadata.\n// IDs are percent-escaped UTF-8 path components.\n// Cancellation wait: rustev-wait-ms metadata. Other headers keep their names.\n// Canonical HTTP body bytes, including empty and OpenMetrics bodies.\nmessage Document { bytes document = 1; }\n\nservice Serve {\n",
    );
    for op in operation_table() {
        proto.push_str(&format!(
            "  // {}\n  rpc {:?}(Document) returns ({}Document);\n",
            serde_json::to_string(&op).unwrap(),
            op.operation,
            if op.streaming { "stream " } else { "" }
        ));
    }
    proto.push_str("}\n");
    std::fs::write(scratch.join("serve.proto"), &proto).unwrap();
    // Bind the committed stub to the exact proto bytes even without protoc.
    // With protoc, also compare the complete regenerated Rust source.
    let digest_header = format!(
        "// serve.proto SHA-256: {:x}\n",
        Sha256::digest(proto.as_bytes())
    );
    let stub_path = root.join("src/generated/rustev.serve.v1.rs");
    let bless = std::env::var("RUSTEV_BLESS").as_deref() == Ok("1");
    let compiler = protoc();
    assert!(
        !bless || compiler.is_some(),
        "RUSTEV_BLESS=1 requires protoc to regenerate both the proto and Rust stub"
    );
    let mut outputs = vec![(
        root.join("proto/rustev/serve/v1/serve.proto"),
        proto.into_bytes(),
    )];
    match compiler {
        Some(protoc) => {
            let mut config = tonic_prost_build::Config::new();
            config.protoc_executable(protoc);
            tonic_prost_build::configure()
                .build_client(false)
                .build_server(true)
                .out_dir(&scratch)
                .compile_with_config(
                    config,
                    &[scratch.join("serve.proto")],
                    std::slice::from_ref(&scratch),
                )
                .unwrap();
            let mut stub = digest_header.as_bytes().to_vec();
            stub.extend(std::fs::read(scratch.join("rustev.serve.v1.rs")).unwrap());
            outputs.push((stub_path.clone(), stub));
        }
        None => eprintln!(
            "Rust stub proto digest checked; full regeneration unavailable without protoc"
        ),
    }
    for (path, bytes) in outputs {
        if bless {
            std::fs::write(&path, &bytes).unwrap();
        }
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "drift: {}",
            path.display()
        );
    }
    let stub = std::fs::read_to_string(&stub_path).unwrap();
    assert_eq!(
        stub.lines().next(),
        Some(digest_header.trim_end()),
        "stale Rust gRPC stub: regenerate with protoc and RUSTEV_BLESS=1"
    );
    std::fs::remove_dir_all(scratch).unwrap();
}

fn protoc() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PROTOC") {
        return Some(PathBuf::from(p));
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join("protoc"))
        .find(|p| p.is_file())
}
