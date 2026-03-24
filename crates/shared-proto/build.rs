use std::path::PathBuf;

fn main() {
    let proto_dir: PathBuf = PathBuf::from("../../schemas/proto");
    let oco_proto = proto_dir.join("oco.proto");
    let orchestrator_proto = proto_dir.join("orchestrator.proto");

    // Re-run if proto files change.
    println!("cargo:rerun-if-changed={}", oco_proto.display());
    println!("cargo:rerun-if-changed={}", orchestrator_proto.display());

    // Attempt compilation with tonic-build (needs protoc in PATH).
    // If protoc is missing, fall back to writing stub modules so the
    // crate still compiles and downstream code can reference the types
    // once protoc becomes available.
    let compile_result = tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .out_dir(out_dir())
        .compile_protos(
            &[&oco_proto, &orchestrator_proto],
            &[&proto_dir],
        );

    match compile_result {
        Ok(()) => {
            println!("cargo:warning=proto compilation succeeded");
        }
        Err(e) => {
            eprintln!("proto compilation failed ({e}), generating stubs");
            generate_stubs();
        }
    }
}

/// Write empty stub modules so that `include!` in lib.rs doesn't break.
fn generate_stubs() {
    let out = out_dir();

    let stubs = [
        ("oco.ml.rs", "// stub: rebuild with protoc to get real types\n"),
        (
            "oco.orchestrator.rs",
            "// stub: rebuild with protoc to get real types\n",
        ),
    ];

    for (name, content) in stubs {
        let path = out.join(name);
        if !path.exists() {
            std::fs::write(&path, content).unwrap_or_else(|e| {
                panic!("failed to write stub {}: {e}", path.display());
            });
        }
    }
}

fn out_dir() -> PathBuf {
    PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"))
}
