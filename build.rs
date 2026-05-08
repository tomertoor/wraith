fn main() -> Result<(), Box<dyn std::error::Error>> {
    prost_build::Config::new()
        .out_dir(std::path::PathBuf::from("src/proto"))
        .compile_protos(&["proto/wraith.proto"], &["proto/"])?;
    Ok(())
}
