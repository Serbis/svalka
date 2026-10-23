fn main() {
    println!("cargo:rerun-if-changed=proto/telemetry.proto");
    println!("cargo:rerun-if-changed=proto/");

    let out_dir = std::path::PathBuf::from("src/proto");
    std::fs::create_dir_all(&out_dir).unwrap();

    prost_build::Config::new()
        .out_dir(out_dir)
        .compile_protos(&["proto/telemetry.proto"], &["proto/"])
        .unwrap();
}